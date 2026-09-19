use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, Read as _, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use fs2::FileExt as _;
use serde::{Deserialize, Serialize};

const INSTANCE_SCHEMA: u32 = 1;
const MAX_MESSAGE_BYTES: u64 = 64 * 1024;
const IO_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenRequest {
    pub paths: Vec<PathBuf>,
}

#[derive(Debug)]
pub enum Startup {
    Primary(PrimaryInstance),
    Forwarded,
}

#[derive(Debug)]
pub struct PrimaryInstance {
    _lock: File,
    receiver: InstanceReceiver,
    endpoint_path: PathBuf,
}

#[derive(Clone, Debug)]
pub struct InstanceReceiver {
    listener: Arc<TcpListener>,
    token: Arc<str>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Endpoint {
    schema: u32,
    address: SocketAddr,
    token: String,
    pid: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Envelope {
    schema: u32,
    token: String,
    request: OpenRequest,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Acknowledgement {
    schema: u32,
    accepted: bool,
}

impl PrimaryInstance {
    pub fn receiver(&self) -> InstanceReceiver {
        self.receiver.clone()
    }
}

impl Drop for PrimaryInstance {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.endpoint_path);
    }
}

impl InstanceReceiver {
    pub fn receive(&self) -> io::Result<OpenRequest> {
        let (mut stream, peer) = self.listener.accept()?;
        if !peer.ip().is_loopback() {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "instance request did not originate on loopback",
            ));
        }
        configure_stream(&stream)?;
        let envelope: Envelope = read_line_json(&mut stream)?;
        let accepted = envelope.schema == INSTANCE_SCHEMA && envelope.token == *self.token;
        write_line_json(
            &mut stream,
            &Acknowledgement {
                schema: INSTANCE_SCHEMA,
                accepted,
            },
        )?;
        if !accepted {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "instance request authentication failed",
            ));
        }
        Ok(envelope.request)
    }
}

pub fn acquire_or_forward(app_data: &Path, paths: Vec<PathBuf>) -> io::Result<Startup> {
    let directory = app_data.join("app-instance");
    fs::create_dir_all(&directory)?;
    let lock_path = directory.join("primary.lock");
    let endpoint_path = directory.join("endpoint.json");
    let lock = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(lock_path)?;

    match lock.try_lock_exclusive() {
        Ok(()) => {
            let listener = TcpListener::bind(("127.0.0.1", 0))?;
            let token = token();
            let endpoint = Endpoint {
                schema: INSTANCE_SCHEMA,
                address: listener.local_addr()?,
                token: token.clone(),
                pid: std::process::id(),
            };
            write_endpoint(&endpoint_path, &endpoint)?;
            Ok(Startup::Primary(PrimaryInstance {
                _lock: lock,
                receiver: InstanceReceiver {
                    listener: Arc::new(listener),
                    token: Arc::from(token),
                },
                endpoint_path,
            }))
        }
        Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
            forward_with_retry(&endpoint_path, OpenRequest { paths })?;
            Ok(Startup::Forwarded)
        }
        Err(error) => Err(error),
    }
}

fn forward_with_retry(path: &Path, request: OpenRequest) -> io::Result<()> {
    let mut last_error = None;
    for _ in 0..20 {
        match read_endpoint(path).and_then(|endpoint| forward(&endpoint, request.clone())) {
            Ok(()) => return Ok(()),
            Err(error) => last_error = Some(error),
        }
        thread::sleep(Duration::from_millis(25));
    }
    Err(last_error.unwrap_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotConnected,
            "primary instance is unavailable",
        )
    }))
}

fn forward(endpoint: &Endpoint, request: OpenRequest) -> io::Result<()> {
    if endpoint.schema != INSTANCE_SCHEMA || !endpoint.address.ip().is_loopback() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid primary instance endpoint",
        ));
    }
    let mut stream = TcpStream::connect_timeout(&endpoint.address, IO_TIMEOUT)?;
    configure_stream(&stream)?;
    write_line_json(
        &mut stream,
        &Envelope {
            schema: INSTANCE_SCHEMA,
            token: endpoint.token.clone(),
            request,
        },
    )?;
    let acknowledgement: Acknowledgement = read_line_json(&mut stream)?;
    if acknowledgement.schema == INSTANCE_SCHEMA && acknowledgement.accepted {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "primary instance rejected the request",
        ))
    }
}

fn configure_stream(stream: &TcpStream) -> io::Result<()> {
    stream.set_read_timeout(Some(IO_TIMEOUT))?;
    stream.set_write_timeout(Some(IO_TIMEOUT))
}

fn write_endpoint(path: &Path, endpoint: &Endpoint) -> io::Result<()> {
    let temporary = path.with_extension(format!("tmp-{}", std::process::id()));
    let result = (|| {
        let mut file = File::create(&temporary)?;
        serde_json::to_writer(&mut file, endpoint).map_err(io::Error::other)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        fs::rename(&temporary, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result
}

fn read_endpoint(path: &Path) -> io::Result<Endpoint> {
    let file = File::open(path)?;
    serde_json::from_reader(file).map_err(io::Error::other)
}

fn write_line_json(stream: &mut TcpStream, value: &impl Serialize) -> io::Result<()> {
    serde_json::to_writer(&mut *stream, value).map_err(io::Error::other)?;
    stream.write_all(b"\n")?;
    stream.flush()
}

fn read_line_json<T: for<'de> Deserialize<'de>>(reader: &mut impl std::io::Read) -> io::Result<T> {
    let mut line = Vec::new();
    let mut reader = BufReader::new(reader.take(MAX_MESSAGE_BYTES + 1));
    let read = reader.read_until(b'\n', &mut line)?;
    if read == 0 || line.len() as u64 > MAX_MESSAGE_BYTES || !line.ends_with(b"\n") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "instance message exceeds the bounded line envelope",
        ));
    }
    serde_json::from_slice(&line).map_err(io::Error::other)
}

fn token() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("{:08x}{nanos:032x}", std::process::id())
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    #[test]
    fn rejects_oversized_messages() {
        let mut bytes = vec![b'x'; MAX_MESSAGE_BYTES as usize + 1];
        bytes.push(b'\n');
        assert_eq!(
            read_line_json::<Envelope>(&mut Cursor::new(bytes))
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidData
        );
    }
}
