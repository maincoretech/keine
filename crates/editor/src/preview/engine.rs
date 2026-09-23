use std::env;
use std::ffi::OsString;
use std::io::{self, BufReader};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use keine_authoring::{
    Capability, ClientCommand, ClientMessage, FrameTransportDescriptor, LifecycleState,
    MAX_DOCUMENT_BYTES, PROTOCOL_VERSION, PreviewInput, SNAPSHOT_CHUNK_BYTES, ServerMessage,
    ServerResponse, ValidationReport, read_message, write_message,
};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const IO_TIMEOUT: Duration = Duration::from_secs(10);
const EXIT_TIMEOUT: Duration = Duration::from_millis(600);
static NEXT_TOKEN: AtomicU64 = AtomicU64::new(1);

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
    writer: TcpStream,
    reader: BufReader<TcpStream>,
    generation: u64,
    next_request: u64,
    closed: bool,
}

impl EngineProcess {
    pub fn launch(engine: &Path, project: &Path, generation: u64) -> io::Result<Self> {
        let listener = TcpListener::bind(("127.0.0.1", 0))?;
        listener.set_nonblocking(true)?;
        let endpoint = listener.local_addr()?;
        let token = launch_token();
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
        stream.set_read_timeout(Some(IO_TIMEOUT))?;
        stream.set_write_timeout(Some(IO_TIMEOUT))?;
        let reader = BufReader::new(stream.try_clone()?);
        let mut process = Self {
            child,
            writer: stream,
            reader,
            generation,
            next_request: 1,
            closed: false,
        };
        match process.request(ClientCommand::Hello {
            protocol_version: PROTOCOL_VERSION,
            token,
            editor_version: env!("CARGO_PKG_VERSION").into(),
        })? {
            ServerResponse::Hello {
                protocol_version,
                capabilities,
                ..
            } if protocol_version == PROTOCOL_VERSION
                && [
                    Capability::Validate,
                    Capability::RawFramePreview,
                    Capability::SourceSnapshots,
                    Capability::RuntimeInput,
                    Capability::SourceCursor,
                    Capability::Lifecycle,
                ]
                .iter()
                .all(|required| capabilities.contains(required)) => {}
            other => return Err(unexpected("compatible hello", &other)),
        }
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

    pub fn start_preview(
        &mut self,
        transport: FrameTransportDescriptor,
        document_revision: u64,
    ) -> io::Result<LifecycleState> {
        self.lifecycle(ClientCommand::StartPreview {
            transport,
            document_revision,
        })
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

    pub fn pause(&mut self) -> io::Result<LifecycleState> {
        self.lifecycle(ClientCommand::Pause)
    }

    pub fn resume(&mut self) -> io::Result<LifecycleState> {
        self.lifecycle(ClientCommand::Resume)
    }

    pub fn stop(&mut self) -> io::Result<LifecycleState> {
        self.lifecycle(ClientCommand::Stop)
    }

    pub fn input(&mut self, document_revision: u64, event: PreviewInput) -> io::Result<()> {
        match self.request(ClientCommand::Input {
            document_revision,
            event,
        })? {
            ServerResponse::InputAccepted {
                document_revision: accepted,
            } if accepted == document_revision => Ok(()),
            other => Err(unexpected("input acknowledgement", &other)),
        }
    }

    pub fn set_execution_cursor(
        &mut self,
        document_revision: u64,
        path: &Path,
        line: usize,
        column: usize,
    ) -> io::Result<(PathBuf, usize, usize)> {
        match self.request(ClientCommand::SetExecutionCursor {
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
            } if accepted == document_revision => Ok((path, line, column)),
            other => Err(unexpected("source location", &other)),
        }
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

    fn lifecycle(&mut self, command: ClientCommand) -> io::Result<LifecycleState> {
        match self.request(command)? {
            ServerResponse::Lifecycle { state } => Ok(state),
            other => Err(unexpected("lifecycle", &other)),
        }
    }

    fn request(&mut self, command: ClientCommand) -> io::Result<ServerResponse> {
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
        let response: ServerMessage = read_message(&mut self.reader)?.ok_or_else(|| {
            io::Error::new(io::ErrorKind::UnexpectedEof, "Engine authoring host exited")
        })?;
        if response.generation != self.generation || response.request_id != request_id {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Engine returned a stale or mismatched authoring response",
            ));
        }
        match response.response {
            ServerResponse::Error { code, message } => Err(io::Error::other(format!(
                "Engine authoring error {code:?}: {message}"
            ))),
            response => Ok(response),
        }
    }
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

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

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
