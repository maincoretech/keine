use std::io::{self, BufReader};
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

use anyhow::{Context, Result};
use keine_authoring::{
    Capability, ClientCommand, ClientMessage, ErrorCode, LifecycleState, PROTOCOL_VERSION,
    ServerMessage, ServerResponse, read_message, write_message,
};
use keine_loader::LoaderRegistry;

use super::bootstrap::{OpenedProject, open_project, validate_project};

struct Session {
    project: Option<OpenedProject>,
    lifecycle: LifecycleState,
}

impl Default for Session {
    fn default() -> Self {
        Self {
            project: None,
            lifecycle: LifecycleState::Stopped,
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
                Capability::NoFramePreview,
                Capability::Lifecycle,
            ],
        },
    )?;

    let generation = hello.generation;
    let mut session = Session::default();
    while let Some(message) = read_message::<ClientMessage>(&mut reader)? {
        if message.generation != generation {
            send_error(
                &mut stream,
                &message,
                ErrorCode::InvalidRequest,
                "stale authoring session generation",
            )?;
            continue;
        }
        let response = handle(&loader, &mut session, &message.command);
        let shutdown = matches!(message.command, ClientCommand::Shutdown);
        match response {
            Ok(response) => send(&mut stream, &message, response)?,
            Err((code, message_text)) => send_error(&mut stream, &message, code, &message_text)?,
        }
        if shutdown {
            break;
        }
    }
    Ok(())
}

fn handle(
    loader: &LoaderRegistry,
    session: &mut Session,
    command: &ClientCommand,
) -> Result<ServerResponse, (ErrorCode, String)> {
    match command {
        ClientCommand::Hello { .. } => Err((
            ErrorCode::InvalidRequest,
            "hello may only be sent once".into(),
        )),
        ClientCommand::OpenProject { path } => match open_project(path, loader) {
            Ok(project) => {
                let root = project.root.clone();
                let title = project.config.title.clone();
                session.project = Some(project);
                session.lifecycle = LifecycleState::Ready;
                Ok(ServerResponse::ProjectOpened { root, title })
            }
            Err(error) => Err((ErrorCode::ProjectOpenFailed, format!("{error:#}"))),
        },
        ClientCommand::CloseProject => {
            session.project = None;
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
        ClientCommand::Start => set_lifecycle(session, LifecycleState::Running),
        ClientCommand::Pause => {
            if session.lifecycle != LifecycleState::Running {
                return Err((
                    ErrorCode::InvalidRequest,
                    "pause requires a running authoring session".into(),
                ));
            }
            set_lifecycle(session, LifecycleState::Paused)
        }
        ClientCommand::Resume => {
            if session.lifecycle != LifecycleState::Paused {
                return Err((
                    ErrorCode::InvalidRequest,
                    "resume requires a paused authoring session".into(),
                ));
            }
            set_lifecycle(session, LifecycleState::Running)
        }
        ClientCommand::Stop => set_lifecycle(session, LifecycleState::Stopped),
        ClientCommand::Ping => Ok(ServerResponse::Pong),
        ClientCommand::Shutdown => Ok(ServerResponse::Bye),
    }
}

fn set_lifecycle(
    session: &mut Session,
    state: LifecycleState,
) -> Result<ServerResponse, (ErrorCode, String)> {
    if session.project.is_none() {
        return Err((ErrorCode::ProjectNotOpen, "no project is open".into()));
    }
    session.lifecycle = state;
    Ok(ServerResponse::Lifecycle { state })
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
