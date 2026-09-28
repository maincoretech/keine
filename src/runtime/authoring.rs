use std::collections::HashMap;
use std::fs;
use std::io::{self, BufReader};
use std::net::{SocketAddr, TcpStream};
use std::path::{Component, Path, PathBuf};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result};
use bevy::prelude::{App, Resource, World};
use bevy::winit::{EventLoopProxyWrapper, WinitUserEvent};
use keine_authoring::{
    Capability, ClientCommand, ClientMessage, ErrorCode, LifecycleState, MAX_DOCUMENT_BYTES,
    PROTOCOL_VERSION, ServerMessage, ServerResponse, preview_overlay_path, read_message,
    write_message,
};
use keine_loader::LoaderRegistry;

use super::bootstrap::{
    OpenedProject, build_authoring_preview_app, open_project, validate_project,
};

type PublishedPosition = (u64, Option<(PathBuf, usize, usize)>);

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
    fn create(token: &str) -> io::Result<Self> {
        let root = preview_overlay_path(token);
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            // Temporary parents can be shared (for example /tmp on Linux).
            // Set owner-only access when creating the save/snapshot root.
            fs::DirBuilder::new().mode(0o700).create(&root)?;
        }
        #[cfg(not(unix))]
        fs::create_dir(&root)?;
        if let Err(error) = fs::create_dir(root.join("scripts")) {
            let _ = fs::remove_dir(&root);
            return Err(error);
        }
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
    document_revision: u64,
    pending_snapshot: Option<PendingSnapshot>,
    documents: HashMap<PathBuf, Vec<u8>>,
    overlay: PreviewOverlay,
    #[cfg(any(feature = "audio-opus", feature = "audio-seekable"))]
    audition: Option<AudioAudition>,
}

impl Session {
    fn new(token: &str) -> io::Result<Self> {
        Ok(Self {
            project: None,
            project_path: None,
            lifecycle: LifecycleState::Stopped,
            document_revision: 0,
            pending_snapshot: None,
            documents: HashMap::new(),
            overlay: PreviewOverlay::create(token)?,
            #[cfg(any(feature = "audio-opus", feature = "audio-seekable"))]
            audition: None,
        })
    }

    fn stop(&mut self) {
        self.stop_audition();
        self.lifecycle = LifecycleState::Stopped;
    }

    fn stop_audition(&mut self) {
        #[cfg(any(feature = "audio-opus", feature = "audio-seekable"))]
        {
            self.audition = None;
        }
    }

    fn audition_path(&self) -> Option<PathBuf> {
        #[cfg(any(feature = "audio-opus", feature = "audio-seekable"))]
        {
            self.audition
                .as_ref()
                .filter(|audition| !audition.player.empty())
                .map(|audition| audition.path.clone())
        }
        #[cfg(not(any(feature = "audio-opus", feature = "audio-seekable")))]
        None
    }

    fn audition_audio(
        &mut self,
        path: Option<&Path>,
    ) -> Result<ServerResponse, (ErrorCode, String)> {
        self.stop_audition();
        let Some(path) = path else {
            return Ok(ServerResponse::AudioAudition { path: None });
        };
        // The Editor sends a project-relative file path. Resolve it into the
        // last matching asset mount before using the runtime's logical decoder.
        // ContentMount remains the owner of file and symlink confinement.
        if path.as_os_str().is_empty()
            || !path
                .components()
                .all(|part| matches!(part, Component::Normal(_)))
        {
            return Err((
                ErrorCode::InvalidRequest,
                "audio asset path must be project-relative".into(),
            ));
        }
        let project = self
            .project
            .as_ref()
            .ok_or((ErrorCode::ProjectNotOpen, "no project is open".into()))?;
        #[cfg(any(feature = "audio-opus", feature = "audio-seekable"))]
        {
            let requested = project.root.join(path);
            let (mount, logical) = project
                .content
                .asset_mounts()
                .into_iter()
                .rev()
                .find_map(|mount| {
                    let root = mount.filesystem_root()?;
                    let logical = requested.strip_prefix(root).ok()?.to_owned();
                    mount.contains_file(&logical).then_some((mount, logical))
                })
                .ok_or((
                    ErrorCode::InvalidRequest,
                    "audio asset not found in project mounts".into(),
                ))?;
            let source = super::audio::authoring_audio_source(vec![mount].into(), &logical)
                .map_err(|error| (ErrorCode::InvalidRequest, error.to_string()))?;
            // Rodio 0.22: both the device sink and Player must live until playback
            // stops. Dropping this transient owner stops sound, including disconnect.
            let mut output = rodio::DeviceSinkBuilder::open_default_sink()
                .map_err(|error| (ErrorCode::Internal, error.to_string()))?;
            output.log_on_drop(false);
            let player = rodio::Player::connect_new(output.mixer());
            player.append(source);
            self.audition = Some(AudioAudition {
                path: path.to_owned(),
                player,
                _output: output,
            });
            Ok(ServerResponse::AudioAudition {
                path: Some(path.to_owned()),
            })
        }
        #[cfg(not(any(feature = "audio-opus", feature = "audio-seekable")))]
        {
            let _ = project;
            let _ = path;
            Err((
                ErrorCode::InvalidRequest,
                "audio audition requires an audio-enabled Engine".into(),
            ))
        }
    }

    fn build_runtime(&self, loader: &LoaderRegistry) -> Result<App> {
        let project_path = self.project_path.as_deref().context("no project is open")?;
        build_authoring_preview_app(
            project_path,
            &self.overlay.root,
            loader,
            super::preview::AuthoringPreviewConfig {
                document_revision: self.document_revision,
            },
        )
    }
}

#[cfg(any(feature = "audio-opus", feature = "audio-seekable"))]
struct AudioAudition {
    path: PathBuf,
    player: rodio::Player,
    _output: rodio::MixerDeviceSink,
}

#[derive(Resource)]
struct LiveHost {
    session: Session,
    messages: Mutex<Receiver<io::Result<Option<ClientMessage>>>>,
    writer: TcpStream,
    generation: u64,
    published_position: Option<PublishedPosition>,
}

fn handle_live_messages(world: &mut World) {
    let Some(mut host) = world.remove_resource::<LiveHost>() else {
        return;
    };
    loop {
        let message = host
            .messages
            .lock()
            .expect("authoring receiver poisoned")
            .try_recv();
        let (message, shutdown) = match message {
            Ok(Ok(Some(message))) => {
                let shutdown = matches!(
                    message.command,
                    ClientCommand::Stop | ClientCommand::Shutdown
                );
                (message, shutdown)
            }
            Ok(Ok(None)) | Ok(Err(_)) | Err(TryRecvError::Disconnected) => {
                world.write_message(bevy::app::AppExit::Success);
                break;
            }
            Err(TryRecvError::Empty) => break,
        };
        let result = if message.generation != host.generation {
            Err((
                ErrorCode::InvalidRequest,
                "stale authoring session generation".into(),
            ))
        } else {
            handle(
                &LoaderRegistry::default(),
                &mut host.session,
                Some(world),
                &message.command,
            )
        };
        let sent = match result {
            Ok(response) => send(&mut host.writer, &message, response),
            Err((code, text)) => send_error(&mut host.writer, &message, code, &text),
        };
        if sent.is_err() || shutdown {
            world.write_message(bevy::app::AppExit::Success);
            break;
        }
    }
    world.insert_resource(host);
}

/// Push only a changed execution location. Idle previews send nothing, so
/// cursor synchronization does not wake the native renderer or poll the GPU.
fn publish_live_position(world: &mut World) {
    let Some(mut host) = world.remove_resource::<LiveHost>() else {
        return;
    };
    let current = (
        host.session.document_revision,
        super::preview::source_location(world),
    );
    if host.published_position.as_ref() != Some(&current) {
        let message = ServerMessage {
            generation: host.generation,
            request_id: 0,
            response: ServerResponse::ExecutionLocation {
                document_revision: current.0,
                location: current.1.clone(),
            },
        };
        if write_message(&mut host.writer, &message).is_err() {
            world.write_message(bevy::app::AppExit::Success);
        }
        host.published_position = Some(current);
    }
    world.insert_resource(host);
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
                Capability::NativePreviewWindow,
                #[cfg(any(feature = "audio-opus", feature = "audio-seekable"))]
                Capability::AudioAudition,
                Capability::SourceSnapshots,
                Capability::SourceCursor,
                Capability::Lifecycle,
            ],
        },
    )?;

    let generation = hello.generation;
    stream.set_read_timeout(None)?;
    let wake = Arc::new(Mutex::new(None));
    let messages = spawn_reader(reader, wake.clone());
    let mut session = Session::new(token)?;
    loop {
        match messages.recv() {
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
                if let ClientCommand::StartPreview { document_revision } = &message.command {
                    if session.project.is_none() {
                        send_error(
                            &mut stream,
                            &message,
                            ErrorCode::ProjectNotOpen,
                            "no project is open",
                        )?;
                        continue;
                    }
                    session.stop_audition();
                    session.document_revision = *document_revision;
                    let mut runtime = match session.build_runtime(&loader) {
                        Ok(runtime) => runtime,
                        Err(error) => {
                            send_error(
                                &mut stream,
                                &message,
                                ErrorCode::Internal,
                                &error.to_string(),
                            )?;
                            continue;
                        }
                    };
                    session.lifecycle = LifecycleState::Running;
                    runtime.insert_resource(LiveHost {
                        session,
                        messages: Mutex::new(messages),
                        writer: stream.try_clone()?,
                        generation,
                        published_position: None,
                    });
                    runtime.add_systems(bevy::prelude::First, handle_live_messages);
                    runtime.add_systems(bevy::prelude::Last, publish_live_position);
                    *wake.lock().expect("authoring wake lock poisoned") = Some(
                        std::ops::Deref::deref(runtime.world().resource::<EventLoopProxyWrapper>())
                            .clone(),
                    );
                    send(
                        &mut stream,
                        &message,
                        ServerResponse::Lifecycle {
                            state: LifecycleState::Running,
                        },
                    )?;
                    // Bevy/winit owns the native event loop on this process's
                    // main thread. Socket reads continue on the existing IO
                    // thread; commands are applied inside the ECS update.
                    runtime.run();
                    return Ok(());
                }
                let shutdown = matches!(message.command, ClientCommand::Shutdown);
                let response = handle(&loader, &mut session, None, &message.command);
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
            Err(_) => break,
        }
    }
    session.stop();
    Ok(())
}

fn spawn_reader(
    mut reader: BufReader<TcpStream>,
    wake: Arc<Mutex<Option<winit::event_loop::EventLoopProxy<WinitUserEvent>>>>,
) -> Receiver<io::Result<Option<ClientMessage>>> {
    let (sender, receiver) = mpsc::sync_channel(8);
    thread::spawn(move || {
        loop {
            let message = read_message::<ClientMessage>(&mut reader);
            let finished = !matches!(message, Ok(Some(_)));
            if sender.send(message).is_err() {
                break;
            }
            if let Some(proxy) = wake.lock().expect("authoring wake lock poisoned").as_ref() {
                let _ = proxy.send_event(WinitUserEvent::WakeUp);
            }
            if finished {
                break;
            }
        }
    });
    receiver
}

fn handle(
    loader: &LoaderRegistry,
    session: &mut Session,
    mut runtime: Option<&mut World>,
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
            commit_snapshot(session, runtime.as_deref_mut(), *revision)
        }
        ClientCommand::ApplyDocumentPatch {
            path,
            base_revision,
            revision,
            start,
            end,
            replacement,
        } => apply_patch(
            session,
            runtime.as_deref_mut(),
            path,
            *base_revision,
            *revision,
            *start,
            *end,
            replacement,
        ),
        ClientCommand::StartPreview { .. } => Err((
            ErrorCode::InvalidRequest,
            "preview is already running".into(),
        )),
        ClientCommand::ShowPreview => {
            let Some(runtime) = runtime else {
                return Err((ErrorCode::InvalidRequest, "preview is not running".into()));
            };
            // Bevy/winit owns activation on each desktop platform. Wayland
            // may deny programmatic focus; the native window remains usable.
            let mut windows = runtime.query_filtered::<
                &mut bevy::window::Window,
                bevy::prelude::With<bevy::window::PrimaryWindow>,
            >();
            for mut window in windows.iter_mut(runtime) {
                window.focused = true;
                window.visible = true;
            }
            Ok(ServerResponse::Lifecycle {
                state: session.lifecycle,
            })
        }
        ClientCommand::AuditionAudio { path } => session.audition_audio(path.as_deref()),
        ClientCommand::AuditionState => Ok(ServerResponse::AudioAudition {
            path: session.audition_path(),
        }),
        ClientCommand::Stop => {
            session.stop();
            Ok(ServerResponse::Lifecycle {
                state: session.lifecycle,
            })
        }
        ClientCommand::SetExecutionCursor {
            document_revision,
            path,
            line,
            column,
        } => {
            require_revision(session, *document_revision)?;
            let Some(runtime) = runtime else {
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
    session: &mut Session,
    runtime: Option<&mut World>,
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
    reload_live_source(runtime, revision);
    Ok(ServerResponse::SnapshotApplied {
        document_revision: revision,
    })
}

#[allow(clippy::too_many_arguments)]
fn apply_patch(
    session: &mut Session,
    runtime: Option<&mut World>,
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
    reload_live_source(runtime, revision);
    Ok(ServerResponse::SnapshotApplied {
        document_revision: revision,
    })
}

fn reload_live_source(runtime: Option<&mut World>, revision: u64) {
    let Some(world) = runtime else {
        return;
    };
    if let Err(error) = super::preview::reload_source(world, revision) {
        // Invalid in-progress text must leave the last valid Program live.
        log::warn!("preview kept the last valid source: {error:#}");
        super::preview::set_document_revision(world, revision);
    }
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
