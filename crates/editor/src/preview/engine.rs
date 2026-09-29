use std::env;
use std::ffi::OsString;
use std::fs;
use std::io::{self, BufReader};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use keine_authoring::{
    Capability, ClientCommand, ClientMessage, ErrorCode, LifecycleState, MAX_DOCUMENT_BYTES,
    PROTOCOL_VERSION, SNAPSHOT_CHUNK_BYTES, ServerMessage, ServerResponse, ValidationReport,
    preview_overlay_path, read_message, write_message,
};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const IO_TIMEOUT: Duration = Duration::from_secs(10);
const EXIT_TIMEOUT: Duration = Duration::from_millis(600);
static NEXT_TOKEN: AtomicU64 = AtomicU64::new(1);
type PublishedPosition = (u64, Option<(PathBuf, usize, usize)>);

#[derive(Clone, Debug)]
pub struct EngineLocator {
    editor_executable: PathBuf,
    explicit: Option<PathBuf>,
}

impl EngineLocator {
    pub fn current() -> io::Result<Self> {
        Ok(Self {
            editor_executable: env::current_exe()?,
            explicit: env::var_os("KEINE_ENGINE").map(PathBuf::from),
        })
    }

    pub fn with_explicit(editor_executable: PathBuf, engine: PathBuf) -> Self {
        Self {
            editor_executable,
            explicit: Some(engine),
        }
    }

    pub fn locate(&self) -> io::Result<PathBuf> {
        let mut candidates = Vec::new();
        if let Some(explicit) = &self.explicit {
            candidates.push(explicit.clone());
        }
        if let Some(directory) = self.editor_executable.parent() {
            candidates.push(directory.join(engine_file_name()));
            if directory.file_name().is_some_and(|name| name == "MacOS") {
                candidates.push(
                    directory
                        .parent()
                        .unwrap_or(directory)
                        .join("Resources")
                        .join(engine_file_name()),
                );
                // Independent macOS installs can place both app bundles in the
                // same Applications directory without embedding an Engine in
                // the Editor bundle.
                if cfg!(target_os = "macos")
                    && let Some(app) = directory.parent().and_then(Path::parent)
                    && app.extension().is_some_and(|extension| extension == "app")
                    && let Some(install_dir) = app.parent()
                {
                    candidates.push(
                        install_dir
                            .join("Kēne Engine.app/Contents/MacOS")
                            .join(engine_file_name()),
                    );
                }
            }
        }
        if let Some(path) = find_on_path(engine_file_name()) {
            candidates.push(path);
        }
        candidates
            .into_iter()
            .find(|candidate| candidate.is_file())
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::NotFound,
                    if cfg!(target_os = "macos") {
                        "Kēne Engine was not found; place Kēne Engine.app beside Editor or set KEINE_ENGINE"
                    } else {
                        "Kēne Engine was not found; place it beside the editor or set KEINE_ENGINE"
                    },
                )
            })
    }
}

pub struct EngineProcess {
    child: Child,
    overlay_root: PathBuf,
    writer: TcpStream,
    responses: mpsc::Receiver<io::Result<ServerMessage>>,
    published_position: Arc<Mutex<Option<PublishedPosition>>>,
    generation: u64,
    next_request: u64,
    closed: bool,
}

impl EngineProcess {
    pub fn id(&self) -> u32 {
        self.child.id()
    }

    pub(crate) fn process_usage(&mut self) -> Option<super::performance::process::ProcessUsage> {
        // Check the owned child before querying its PID; never sample an unrelated
        // process after this child has exited and its PID becomes reusable.
        if self.closed || self.child.try_wait().ok()?.is_some() {
            return None;
        }
        super::performance::process::read(self.child.id())
    }

    pub fn launch(engine: &Path, project: &Path, generation: u64) -> io::Result<Self> {
        let listener = TcpListener::bind(("127.0.0.1", 0))?;
        listener.set_nonblocking(true)?;
        let endpoint = listener.local_addr()?;
        let token = launch_token();
        let overlay_root = preview_overlay_path(&token);
        let mut child = Command::new(engine)
            .arg("__authoring-host")
            .arg(endpoint.to_string())
            .arg(&token)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;

        let stream = accept_child(&listener, &mut child)?;
        stream.set_nodelay(true)?;
        stream.set_write_timeout(Some(IO_TIMEOUT))?;
        let reader = BufReader::new(stream.try_clone()?);
        let (responses, published_position) = spawn_response_reader(reader, generation);
        let mut process = Self {
            child,
            overlay_root,
            writer: stream,
            responses,
            published_position,
            generation,
            next_request: 1,
            closed: false,
        };
        let hello = process.request_raw(ClientCommand::Hello {
            protocol_version: PROTOCOL_VERSION,
            token,
            editor_version: env!("CARGO_PKG_VERSION").into(),
        })?;
        require_compatible_hello(hello)?;
        match process.request(ClientCommand::OpenProject {
            path: project.to_owned(),
        })? {
            ServerResponse::ProjectOpened { .. } => Ok(process),
            other => Err(unexpected("project_opened", &other)),
        }
    }

    pub fn ping(&mut self) -> io::Result<()> {
        match self.request(ClientCommand::Ping)? {
            ServerResponse::Pong => Ok(()),
            other => Err(unexpected("pong", &other)),
        }
    }

    pub fn validate(&mut self) -> io::Result<ValidationReport> {
        match self.request(ClientCommand::Validate)? {
            ServerResponse::Validation(report) => Ok(report),
            other => Err(unexpected("validation", &other)),
        }
    }

    pub fn apply_snapshot(
        &mut self,
        path: &Path,
        revision: u64,
        contents: &[u8],
    ) -> io::Result<()> {
        if contents.len() > MAX_DOCUMENT_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "preview document exceeds the 1 MiB protocol limit",
            ));
        }
        match self.request(ClientCommand::BeginDocumentSnapshot {
            path: path.to_owned(),
            revision,
            total_bytes: contents.len() as u32,
        })? {
            ServerResponse::SnapshotApplied { document_revision }
                if document_revision == revision => {}
            other => return Err(unexpected("snapshot begin", &other)),
        }
        for (index, bytes) in contents.chunks(SNAPSHOT_CHUNK_BYTES).enumerate() {
            match self.request(ClientCommand::AppendDocumentSnapshot {
                revision,
                offset: (index * SNAPSHOT_CHUNK_BYTES) as u32,
                bytes: bytes.to_vec(),
            })? {
                ServerResponse::SnapshotApplied { document_revision }
                    if document_revision == revision => {}
                other => return Err(unexpected("snapshot chunk", &other)),
            }
        }
        match self.request(ClientCommand::CommitDocumentSnapshot { revision })? {
            ServerResponse::SnapshotApplied { document_revision }
                if document_revision == revision =>
            {
                Ok(())
            }
            other => Err(unexpected("snapshot commit", &other)),
        }
    }

    pub fn start_preview(&mut self, document_revision: u64) -> io::Result<LifecycleState> {
        self.lifecycle(ClientCommand::StartPreview { document_revision })
    }

    pub fn audition_audio(&mut self, path: Option<PathBuf>) -> io::Result<Option<PathBuf>> {
        match self.request(ClientCommand::AuditionAudio { path })? {
            ServerResponse::AudioAudition { path } => Ok(path),
            other => Err(unexpected("audio audition", &other)),
        }
    }

    pub fn audition_state(&mut self) -> io::Result<Option<PathBuf>> {
        match self.request(ClientCommand::AuditionState)? {
            ServerResponse::AudioAudition { path } => Ok(path),
            other => Err(unexpected("audio audition", &other)),
        }
    }

    pub fn show_preview(&mut self) -> io::Result<LifecycleState> {
        self.lifecycle(ClientCommand::ShowPreview)
    }

    pub fn apply_patch(
        &mut self,
        path: &Path,
        base_revision: u64,
        revision: u64,
        range: std::ops::Range<usize>,
        replacement: &[u8],
    ) -> io::Result<()> {
        let start = u32::try_from(range.start).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "preview patch start exceeds u32",
            )
        })?;
        let end = u32::try_from(range.end).map_err(|_| {
            io::Error::new(io::ErrorKind::InvalidInput, "preview patch end exceeds u32")
        })?;
        match self.request(ClientCommand::ApplyDocumentPatch {
            path: path.to_owned(),
            base_revision,
            revision,
            start,
            end,
            replacement: replacement.to_vec(),
        })? {
            ServerResponse::SnapshotApplied { document_revision }
                if document_revision == revision =>
            {
                Ok(())
            }
            other => Err(unexpected("document patch", &other)),
        }
    }

    pub fn stop(&mut self) -> io::Result<LifecycleState> {
        let state = self.lifecycle(ClientCommand::Stop)?;
        self.closed = true;
        wait_or_kill(&mut self.child)?;
        Ok(state)
    }

    pub fn set_execution_cursor(
        &mut self,
        document_revision: u64,
        path: &Path,
        line: usize,
        column: usize,
    ) -> io::Result<Option<(PathBuf, usize, usize)>> {
        match self.request_raw(ClientCommand::SetExecutionCursor {
            document_revision,
            path: path.to_owned(),
            line,
            column,
        })? {
            ServerResponse::SourceLocation {
                document_revision: accepted,
                path,
                line,
                column,
            } if accepted == document_revision => Ok(Some((path, line, column))),
            ServerResponse::Error {
                code: ErrorCode::InvalidRequest,
                ..
            } => Ok(None),
            ServerResponse::Error {
                code: ErrorCode::ProtocolMismatch,
                message,
            } => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("{message}. Update Kēne Editor and Engine together"),
            )),
            ServerResponse::Error { code, message } => Err(io::Error::other(format!(
                "Engine authoring error {code:?}: {message}"
            ))),
            other => Err(unexpected("source location", &other)),
        }
    }

    /// Receive only an Engine-published change. This never sends a request or
    /// wakes an otherwise idle native render loop.
    pub fn take_published_position(
        &mut self,
        revision: u64,
    ) -> io::Result<Option<Option<(PathBuf, usize, usize)>>> {
        if let Some(status) = self.child.try_wait()? {
            return Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                format!("Engine preview exited: {status}"),
            ));
        }
        Ok(self
            .published_position
            .lock()
            .expect("preview position lock poisoned")
            .take()
            .and_then(|(received, position)| (received == revision).then_some(position)))
    }

    pub fn shutdown(&mut self) -> io::Result<()> {
        if self.closed {
            return Ok(());
        }
        match self.request(ClientCommand::Shutdown)? {
            ServerResponse::Bye => {
                self.closed = true;
                wait_or_kill(&mut self.child)
            }
            other => Err(unexpected("bye", &other)),
        }
    }

    pub fn kill_for_test(&mut self) -> io::Result<()> {
        self.closed = true;
        self.child.kill()?;
        self.child.wait().map(|_| ())
    }

    pub fn preview_data_root(&self) -> PathBuf {
        self.overlay_root.join("preview-data")
    }

    fn lifecycle(&mut self, command: ClientCommand) -> io::Result<LifecycleState> {
        match self.request(command)? {
            ServerResponse::Lifecycle { state } => Ok(state),
            other => Err(unexpected("lifecycle", &other)),
        }
    }

    fn request(&mut self, command: ClientCommand) -> io::Result<ServerResponse> {
        match self.request_raw(command)? {
            ServerResponse::Error { code, message } => Err(io::Error::other(format!(
                "Engine authoring error {code:?}: {message}"
            ))),
            response => Ok(response),
        }
    }

    fn request_raw(&mut self, command: ClientCommand) -> io::Result<ServerResponse> {
        let request_id = self.next_request;
        self.next_request = self.next_request.wrapping_add(1).max(1);
        write_message(
            &mut self.writer,
            &ClientMessage {
                generation: self.generation,
                request_id,
                command,
            },
        )?;
        let response = self
            .responses
            .recv_timeout(IO_TIMEOUT)
            .map_err(|error| match error {
                mpsc::RecvTimeoutError::Timeout => io::Error::new(
                    io::ErrorKind::TimedOut,
                    "Engine authoring response timed out",
                ),
                mpsc::RecvTimeoutError::Disconnected => {
                    io::Error::new(io::ErrorKind::UnexpectedEof, "Engine authoring host exited")
                }
            })??;
        if response.generation != self.generation || response.request_id != request_id {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Engine returned a stale or mismatched authoring response",
            ));
        }
        Ok(response.response)
    }
}

fn spawn_response_reader(
    mut reader: BufReader<TcpStream>,
    generation: u64,
) -> (
    mpsc::Receiver<io::Result<ServerMessage>>,
    Arc<Mutex<Option<PublishedPosition>>>,
) {
    let (sender, responses) = mpsc::channel();
    let published_position = Arc::new(Mutex::new(None));
    let latest = published_position.clone();
    thread::Builder::new()
        .name("keine-preview-replies".into())
        .spawn(move || {
            loop {
                match read_message::<ServerMessage>(&mut reader) {
                    Ok(Some(ServerMessage {
                        generation: event_generation,
                        request_id: 0,
                        response:
                            ServerResponse::ExecutionLocation {
                                document_revision,
                                location,
                            },
                        ..
                    })) => {
                        if event_generation != generation {
                            let _ = sender.send(Err(io::Error::new(
                                io::ErrorKind::InvalidData,
                                "Engine published a stale preview session",
                            )));
                            break;
                        }
                        *latest.lock().expect("preview position lock poisoned") =
                            Some((document_revision, location));
                    }
                    Ok(Some(response)) => {
                        if sender.send(Ok(response)).is_err() {
                            break;
                        }
                    }
                    Ok(None) => {
                        let _ = sender.send(Err(io::Error::new(
                            io::ErrorKind::UnexpectedEof,
                            "Engine authoring host exited",
                        )));
                        break;
                    }
                    Err(error) => {
                        let _ = sender.send(Err(error));
                        break;
                    }
                }
            }
        })
        .expect("preview reply reader must be spawnable");
    (responses, published_position)
}

impl Drop for EngineProcess {
    fn drop(&mut self) {
        if !self.closed {
            let _ = self.shutdown();
        }
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
        // The Engine removes this on clean exit. Reap the child first so a
        // crashed preview cannot leave save slots or snapshots in temp storage.
        let _ = fs::remove_dir_all(&self.overlay_root);
    }
}

fn accept_child(listener: &TcpListener, child: &mut Child) -> io::Result<TcpStream> {
    let deadline = Instant::now() + CONNECT_TIMEOUT;
    loop {
        match listener.accept() {
            Ok((stream, peer)) if peer.ip().is_loopback() => {
                stream.set_nonblocking(false)?;
                return Ok(stream);
            }
            Ok(_) => continue,
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
            Err(error) => return Err(error),
        }
        if let Some(status) = child.try_wait()? {
            return Err(io::Error::other(format!(
                "Engine authoring host exited before connecting: {status}"
            )));
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "Engine authoring host did not connect within 5 seconds",
            ));
        }
        thread::sleep(Duration::from_millis(10));
    }
}

fn wait_or_kill(child: &mut Child) -> io::Result<()> {
    let deadline = Instant::now() + EXIT_TIMEOUT;
    loop {
        if child.try_wait()?.is_some() {
            return Ok(());
        }
        if Instant::now() >= deadline {
            child.kill()?;
            child.wait()?;
            return Ok(());
        }
        thread::sleep(Duration::from_millis(10));
    }
}

fn launch_token() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let sequence = NEXT_TOKEN.fetch_add(1, Ordering::Relaxed);
    format!("{:x}-{:x}-{:x}", std::process::id(), now, sequence)
}

fn engine_file_name() -> OsString {
    if cfg!(windows) {
        "keine.exe".into()
    } else {
        "keine".into()
    }
}

fn find_on_path(name: OsString) -> Option<PathBuf> {
    env::split_paths(&env::var_os("PATH")?)
        .map(|directory| directory.join(&name))
        .find(|candidate| candidate.is_file())
}

fn unexpected(expected: &str, actual: &ServerResponse) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!("expected {expected} from Engine, received {actual:?}"),
    )
}

fn require_compatible_hello(response: ServerResponse) -> io::Result<()> {
    if let ServerResponse::Error {
        code: ErrorCode::ProtocolMismatch,
        message,
    } = response
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("{message}. Update both apps together"),
        ));
    }
    let ServerResponse::Hello {
        protocol_version,
        capabilities,
        ..
    } = response
    else {
        return Err(unexpected("compatible hello", &response));
    };
    if protocol_version != PROTOCOL_VERSION {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "Engine authoring protocol {protocol_version} is incompatible with Editor protocol {PROTOCOL_VERSION}. Update both apps together"
            ),
        ));
    }
    let missing: Vec<_> = [
        Capability::Validate,
        Capability::NativePreviewWindow,
        Capability::SourceSnapshots,
        Capability::SourceCursor,
        Capability::Lifecycle,
    ]
    .into_iter()
    .filter(|required| !capabilities.contains(required))
    .collect();
    if !missing.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "Engine is missing required authoring capabilities {missing:?}. Update both apps together"
            ),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::net::TcpListener;

    use super::*;

    #[test]
    fn position_event_is_latest_wins_and_does_not_consume_a_reply() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let mut writer = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (reader, _) = listener.accept().unwrap();
        let (responses, latest) = spawn_response_reader(BufReader::new(reader), 7);
        for line in [3, 5] {
            write_message(
                &mut writer,
                &ServerMessage {
                    generation: 7,
                    request_id: 0,
                    response: ServerResponse::ExecutionLocation {
                        document_revision: 2,
                        location: Some((PathBuf::from("scripts/main.shou"), line, 1)),
                    },
                },
            )
            .unwrap();
        }
        write_message(
            &mut writer,
            &ServerMessage {
                generation: 7,
                request_id: 1,
                response: ServerResponse::Pong,
            },
        )
        .unwrap();
        assert!(matches!(
            responses
                .recv_timeout(Duration::from_secs(1))
                .unwrap()
                .unwrap()
                .response,
            ServerResponse::Pong
        ));
        assert_eq!(
            *latest.lock().unwrap(),
            Some((2, Some((PathBuf::from("scripts/main.shou"), 5, 1))))
        );
    }

    #[test]
    fn handshake_rejects_mismatched_version_and_missing_capability() {
        let hello = |protocol_version, capabilities| ServerResponse::Hello {
            protocol_version,
            engine_version: "test".into(),
            build_id: "test".into(),
            project_formats: vec!["native".into()],
            capabilities,
        };
        let mismatch = require_compatible_hello(hello(PROTOCOL_VERSION + 1, vec![]))
            .unwrap_err()
            .to_string();
        assert!(mismatch.contains("Update both apps together"));
        let rejected = require_compatible_hello(ServerResponse::Error {
            code: ErrorCode::ProtocolMismatch,
            message: "editor protocol is incompatible".into(),
        })
        .unwrap_err()
        .to_string();
        assert!(rejected.contains("Update both apps together"));
        let missing = require_compatible_hello(hello(PROTOCOL_VERSION, vec![]))
            .unwrap_err()
            .to_string();
        assert!(missing.contains("NativePreviewWindow"));
        assert!(missing.contains("Update both apps together"));
    }

    #[test]
    fn locator_prefers_an_explicit_existing_engine() {
        let root = env::temp_dir().join(format!("keine-engine-locator-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let engine = root.join(engine_file_name());
        fs::write(&engine, b"engine").unwrap();
        let locator = EngineLocator::with_explicit(root.join("editor"), engine.clone());
        assert_eq!(locator.locate().unwrap(), engine);
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn locator_finds_independently_installed_engine_app() {
        let root = env::temp_dir().join(format!(
            "keine-engine-app-locator-{}-{}",
            std::process::id(),
            NEXT_TOKEN.fetch_add(1, Ordering::Relaxed)
        ));
        let editor = root.join("Kēne Editor.app/Contents/MacOS/editor");
        let engine = root.join("Kēne Engine.app/Contents/MacOS/keine");
        fs::create_dir_all(editor.parent().unwrap()).unwrap();
        fs::create_dir_all(engine.parent().unwrap()).unwrap();
        fs::write(&engine, b"engine").unwrap();

        let locator = EngineLocator {
            editor_executable: editor,
            explicit: None,
        };
        assert_eq!(locator.locate().unwrap(), engine);
        fs::remove_dir_all(root).unwrap();
    }
}
