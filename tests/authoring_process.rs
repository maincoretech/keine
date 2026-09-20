use std::io::BufReader;
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use keine_authoring::{
    ClientCommand, ClientMessage, ErrorCode, LifecycleState, PROTOCOL_VERSION, ServerMessage,
    ServerResponse, read_message, write_message,
};
use keine_editor::engine::EngineProcess;

fn engine() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_keine"))
}

fn project() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("projects/test-project")
}

#[test]
fn editor_and_engine_complete_the_phase3_control_lifecycle() {
    let mut process = EngineProcess::launch(&engine(), &project(), 41).unwrap();
    process.ping().unwrap();
    let report = process.validate().unwrap();
    assert_eq!(report.errors, 0, "{:#?}", report.diagnostics);
    assert!(report.scenes > 0);
    assert_eq!(process.start().unwrap(), LifecycleState::Running);
    assert_eq!(process.pause().unwrap(), LifecycleState::Paused);
    assert_eq!(process.resume().unwrap(), LifecycleState::Running);
    assert_eq!(process.stop().unwrap(), LifecycleState::Stopped);
    process.shutdown().unwrap();
}

#[test]
fn one_crashed_engine_does_not_break_another_project_session() {
    let mut first = EngineProcess::launch(&engine(), &project(), 101).unwrap();
    let mut second = EngineProcess::launch(&engine(), &project(), 102).unwrap();
    first.kill_for_test().unwrap();
    second.ping().unwrap();
    second.shutdown().unwrap();
}

#[test]
fn incompatible_protocol_returns_a_structured_error() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let endpoint = listener.local_addr().unwrap();
    let token = "protocol-mismatch-test";
    let mut child = Command::new(engine())
        .arg("__authoring-host")
        .arg(endpoint.to_string())
        .arg(token)
        .spawn()
        .unwrap();
    let (mut stream, _) = listener.accept().unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    write_message(
        &mut stream,
        &ClientMessage {
            generation: 8,
            request_id: 1,
            command: ClientCommand::Hello {
                protocol_version: PROTOCOL_VERSION + 1,
                token: token.into(),
                editor_version: "test".into(),
            },
        },
    )
    .unwrap();
    let response: ServerMessage = read_message(&mut BufReader::new(stream)).unwrap().unwrap();
    assert!(matches!(
        response.response,
        ServerResponse::Error {
            code: ErrorCode::ProtocolMismatch,
            ..
        }
    ));
    assert!(!child.wait().unwrap().success());
}
