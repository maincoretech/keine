use std::net::{TcpListener, TcpStream};
use std::process::Command;
use std::time::Duration;

const CHILD_ENV: &str = "KEINE_EDITOR_PHASE0_IPC_CHILD";
const ADDRESS_ENV: &str = "KEINE_EDITOR_PHASE0_IPC_ADDRESS";
const TOKEN_ENV: &str = "KEINE_EDITOR_PHASE0_IPC_TOKEN";

#[test]
fn phase0_ipc_child() {
    if std::env::var_os(CHILD_ENV).is_none() {
        return;
    }
    let address = std::env::var(ADDRESS_ENV).expect("child address is missing");
    let token = std::env::var(TOKEN_ENV).expect("child token is missing");
    let stream = TcpStream::connect(address).expect("child failed to connect");
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    stream
        .set_write_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    keine_editor::ipc::run_client(stream, &token).expect("child protocol failed");
}

#[test]
fn hello_ping_and_shutdown_cross_a_real_process_boundary() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("failed to bind loopback listener");
    listener.set_nonblocking(false).unwrap();
    let address = listener.local_addr().unwrap();
    let token = format!("phase0-{}", std::process::id());
    let mut child = Command::new(std::env::current_exe().unwrap())
        .arg("phase0_ipc_child")
        .arg("--nocapture")
        .env(CHILD_ENV, "1")
        .env(ADDRESS_ENV, address.to_string())
        .env(TOKEN_ENV, &token)
        .spawn()
        .expect("failed to spawn IPC child");

    let (stream, peer) = listener.accept().expect("failed to accept IPC child");
    assert!(peer.ip().is_loopback());
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    stream
        .set_write_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    keine_editor::ipc::serve(stream, &token).expect("host protocol failed");

    let status = child.wait().expect("failed to wait for IPC child");
    assert!(status.success(), "IPC child exited with {status}");
}
