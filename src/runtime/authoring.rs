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
    PROTOCOL_VERSION, PreviewReload, ServerMessage, ServerResponse, preview_overlay_path,
    read_message, write_message,
};
use keine_loader::LoaderRegistry;

use super::bootstrap::{
    OpenedProject, build_authoring_preview_app, open_project, validate_project,
};

type PublishedPosition = (
    u64,
    Option<(PathBuf, usize, usize)>,
    keine_authoring::RuntimeSnapshot,
);

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
    #[cfg(any(feature = "audio-opus", feature = "audio-seekable"))]
    audio_output: std::sync::OnceLock<bevy::audio::AudioOutput>,
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
            #[cfg(any(feature = "audio-opus", feature = "audio-seekable"))]
            audio_output: std::sync::OnceLock::new(),
        })
    }

    fn stop(&mut self) {
        self.stop_audition();
        #[cfg(any(feature = "audio-opus", feature = "audio-seekable"))]
        self.audio_output.take();
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

    fn pause_audition(&mut self, paused: bool) -> Result<ServerResponse, (ErrorCode, String)> {
        #[cfg(any(feature = "audio-opus", feature = "audio-seekable"))]
        if let Some(audition) = &self.audition {
            if paused {
                audition.player.pause();
            } else {
                audition.player.play();
            }
        }
        #[cfg(not(any(feature = "audio-opus", feature = "audio-seekable")))]
        let _ = paused;
        Ok(ServerResponse::AudioAudition {
            path: self.audition_path(),
        })
    }

    fn audition_audio(
        &mut self,
        path: Option<&Path>,
        gain_db: f32,
    ) -> Result<ServerResponse, (ErrorCode, String)> {
        if !keine_core::config::valid_audio_gain(gain_db) {
            return Err((
                ErrorCode::InvalidRequest,
                "gain_db must be between -60 and 12 dB".into(),
            ));
        }
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
            // Audition and Preview retain the same recovery owner. A missing
            // device queues playback until it returns instead of failing the IPC request.
            let output = self
                .audio_output
                .get_or_init(bevy::audio::AudioOutput::default);
            let player = rodio::Player::connect_new(output.mixer());
            player.set_volume(keine_core::config::audio_gain_linear(gain_db));
            player.append(source);
            self.audition = Some(AudioAudition {
                path: path.to_owned(),
                player,
                _output: output.clone(),
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
        let mut app = build_authoring_preview_app(
            project_path,
            &self.overlay.root,
            loader,
            super::preview::AuthoringPreviewConfig {
                document_revision: self.document_revision,
            },
        )?;
        #[cfg(any(feature = "audio-opus", feature = "audio-seekable"))]
        {
            let output = self
                .audio_output
                .get_or_init(|| app.world().resource::<bevy::audio::AudioOutput>().clone())
                .clone();
            app.insert_resource(output);
        }
        super::audio::configure_audio_wake(&mut app);
        Ok(app)
    }
}

#[cfg(any(feature = "audio-opus", feature = "audio-seekable"))]
struct AudioAudition {
    path: PathBuf,
    player: rodio::Player,
    _output: bevy::audio::AudioOutput,
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
    let Some(applied_revision) = super::preview::document_revision(world) else {
        world.insert_resource(host);
        return;
    };
    let location = super::preview::source_location(world);
    let current = (
        applied_revision,
        location,
        super::preview::runtime_snapshot(world),
    );
    if host.published_position.as_ref() != Some(&current) {
        let message = ServerMessage {
            generation: host.generation,
            request_id: 0,
            response: ServerResponse::ExecutionLocation {
                document_revision: current.0,
                location: current.1.clone(),
                snapshot: current.2.clone(),
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
                Capability::RuntimeInspection,
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
        ClientCommand::AuditionAudio { path, gain_db } => {
            session.audition_audio(path.as_deref(), *gain_db)
        }
        ClientCommand::PauseAudition { paused } => session.pause_audition(*paused),
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
            if super::preview::document_revision(runtime) != Some(*document_revision) {
                return Err((
                    ErrorCode::InvalidRequest,
                    "preview still uses the last valid source; fix script errors before seeking"
                        .into(),
                ));
            }
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
        ClientCommand::SetPreviewVariable {
            document_revision,
            name,
            global,
            expected,
            value,
        } => {
            require_revision(session, *document_revision)?;
            let runtime =
                runtime.ok_or((ErrorCode::InvalidRequest, "Preview is not running".into()))?;
            if super::preview::document_revision(runtime) != Some(*document_revision) {
                return Err((
                    ErrorCode::InvalidRequest,
                    "Fix script errors before changing variables".into(),
                ));
            }
            super::preview::set_variable(runtime, name, *global, expected, value)
                .map_err(|message| (ErrorCode::InvalidRequest, message))?;
            Ok(ServerResponse::Pong)
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
        reload: None,
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
        reload: None,
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
    let reload = reload_live_source(runtime, revision);
    Ok(ServerResponse::SnapshotApplied {
        document_revision: revision,
        reload,
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
    let reload = reload_live_source(runtime, revision);
    Ok(ServerResponse::SnapshotApplied {
        document_revision: revision,
        reload,
    })
}

fn reload_live_source(runtime: Option<&mut World>, revision: u64) -> Option<PreviewReload> {
    let world = runtime?;
    Some(match super::preview::reload_source(world, revision) {
        Ok(true) => PreviewReload::Restarted,
        Ok(false) => PreviewReload::Preserved,
        Err(error) => {
            // One bounded reply per source edit; no frame polling or extra parse.
            log::warn!("preview kept the last valid source: {error:#}");
            let diagnostic = error
                .downcast_ref::<super::tick::reload::ReloadDiagnostic>()
                .and_then(|error| {
                    Some(keine_authoring::Diagnostic {
                        level: keine_authoring::DiagnosticLevel::Error,
                        path: confined_script_path(&if error.path.starts_with("scripts") {
                            error.path.clone()
                        } else {
                            Path::new("scripts").join(&error.path)
                        })?,
                        line: error.diagnostic.span.line,
                        column: error.diagnostic.span.column,
                        message: bounded_reload_message(&error.diagnostic.message),
                    })
                });
            PreviewReload::Rejected {
                message: bounded_reload_message(&format!("{error:#}")),
                diagnostic,
            }
        }
    })
}

fn bounded_reload_message(message: &str) -> String {
    message[..message.floor_char_boundary(4096.min(message.len()))].to_owned()
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
    fn live_reload_rejects_errors_tracks_applied_revision_and_keeps_noop_state() {
        use crate::runtime::{preview, resources::*};
        use crate::scene::images::ImageRoleRegistry;
        use crate::storage::save::ContinuationCheckpoint;
        use bevy::prelude::*;
        use keine_core::{State, Value, config::GameConfig};
        use std::sync::Arc;

        let mut session = Session::new("reload-regression").unwrap();
        let root = &session.overlay.root;
        fs::create_dir_all(root.join("scripts")).unwrap();
        let path = root.join("scripts/main.shou");
        let source = "scene main {\n background(room),\n let score = 0,\n score += 1,\n \"old\",\n call(helper)\n}\nscene helper { \"helper\" }\n";
        fs::write(&path, source).unwrap();
        let config = GameConfig {
            adapter: keine_core::config::AdapterConfig {
                script: "keine".into(),
                ..Default::default()
            },
            script: keine_core::config::ScriptConfig {
                entry: "main".into(),
                ..Default::default()
            },
            ..Default::default()
        };
        let content =
            keine_loader::load_project(root, &[keine_core::config::AssetSourceConfig::default()])
                .unwrap();
        let mut app = App::new();
        app.add_plugins(preview::AuthoringPreviewPlugin::new(
            preview::AuthoringPreviewConfig {
                document_revision: 0,
            },
        ))
        .insert_resource(GameState(State::new()))
        .insert_resource(ContentProjectResource(content))
        .insert_resource(ScriptLanguages(
            LoaderRegistry::default().languages("keine").unwrap(),
        ))
        .insert_resource(GameConfigResource(config))
        .init_resource::<LocalAssetManifest>()
        .init_resource::<ImageRoleRegistry>()
        .init_resource::<ContinuationCheckpoint>();
        let world = app.world_mut();
        preview::reload_source(world, 1).unwrap();
        assert_eq!(preview::document_revision(world), Some(1));
        assert_eq!(
            world.resource::<GameState>().vars.get("score"),
            Some(&Value::Int(1))
        );
        // Simulate live presentation progress that must survive a comment edit.
        world.resource_mut::<GameState>().wait_remaining = 0.42;
        let old = world.resource::<GameState>().0.clone();
        let checkpoint = world.resource::<ContinuationCheckpoint>().generation();
        assert!(
            !world.resource::<LocalAssetManifest>()["main"]
                .resources
                .is_empty()
        );
        assert!(
            !world.resource::<LocalAssetManifest>()["main"]
                .sub_scenes
                .is_empty()
        );
        world
            .resource_mut::<preview::AuthoringPreviewSession>()
            .settle_until = None;
        world.clear_trackers();
        fs::write(&path, format!("// comment\n{source}")).unwrap();
        assert_eq!(
            reload_live_source(Some(world), 2),
            Some(PreviewReload::Preserved)
        );
        assert_eq!(preview::document_revision(world), Some(2));
        assert_eq!(world.resource::<GameState>().0, old);
        assert!(Arc::ptr_eq(
            &world.resource::<GameState>().program,
            &old.program
        ));
        assert_eq!(
            world.resource::<ContinuationCheckpoint>().generation(),
            checkpoint
        );
        assert_eq!(preview::source_location(world).unwrap().1, 6);
        assert!(!world.get_resource_ref::<GameState>().unwrap().is_changed());
        assert!(
            !world
                .get_resource_ref::<GameConfigResource>()
                .unwrap()
                .is_changed()
        );
        assert!(
            !world
                .get_resource_ref::<ImageRoleRegistry>()
                .unwrap()
                .is_changed()
        );
        assert!(
            !world
                .get_resource_ref::<LocalAssetManifest>()
                .unwrap()
                .is_changed()
        );
        assert!(
            world
                .resource::<preview::AuthoringPreviewSession>()
                .settle_until
                .is_none()
        );

        let spans = world
            .resource::<LocalAssetManifest>()
            .get("main")
            .unwrap()
            .action_spans
            .clone();
        // Parse errors, entry-flow errors, missing entry and reference errors
        // must all leave both the live Program and its source map intact.
        for invalid in [
            "scene main { let a = ) == 1 }",
            "scene main { return() }",
            "scene other { \"other\" }",
            "scene main { goto(missing) }",
        ] {
            fs::write(&path, invalid).unwrap();
            session.document_revision = 3;
            let result = reload_live_source(Some(world), 3);
            let Some(PreviewReload::Rejected {
                message,
                diagnostic,
            }) = result
            else {
                panic!("invalid source accepted: {invalid}")
            };
            assert!(!message.is_empty());
            if invalid.contains("let a") {
                let diagnostic = diagnostic.expect("syntax error has a navigable source location");
                assert_eq!(diagnostic.path, PathBuf::from("scripts/main.shou"));
                assert!(diagnostic.line > 0 && diagnostic.column > 0);
            }
            assert_eq!(preview::document_revision(world), Some(2), "{invalid}");
            assert_eq!(world.resource::<GameState>().0, old, "{invalid}");
            assert_eq!(
                world
                    .resource::<LocalAssetManifest>()
                    .get("main")
                    .unwrap()
                    .action_spans,
                spans
            );
            assert_eq!(
                world.resource::<ContinuationCheckpoint>().generation(),
                checkpoint
            );
            let result = handle(
                &LoaderRegistry::default(),
                &mut session,
                Some(world),
                &ClientCommand::SetExecutionCursor {
                    document_revision: 3,
                    path: "scripts/main.shou".into(),
                    line: 1,
                    column: 1,
                },
            );
            assert!(matches!(result, Err((ErrorCode::InvalidRequest, _))));
        }
        fs::write(&path, source.replace("old", "fixed")).unwrap();
        assert_eq!(
            reload_live_source(Some(world), 4),
            Some(PreviewReload::Restarted)
        );
        assert_eq!(preview::document_revision(world), Some(4));
        assert_eq!(
            world.resource::<GameState>().vars.get("score"),
            Some(&Value::Int(1))
        );
        assert_eq!(
            world
                .resource::<GameState>()
                .dialogue
                .as_ref()
                .unwrap()
                .text,
            "fixed"
        );
        assert!(world.resource::<ContinuationCheckpoint>().generation() > checkpoint);
    }

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
