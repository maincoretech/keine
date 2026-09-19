use std::io::{self, BufRead, BufReader, Read, Write};

pub const PROTOCOL_VERSION: u32 = 1;
const MAX_CONTROL_LINE_BYTES: usize = 4 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Request {
    Hello { version: u32, token: String },
    Ping { request_id: u64 },
    Shutdown,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Response {
    Hello { version: u32 },
    Pong { request_id: u64 },
    Bye,
    Error(String),
}

pub fn write_request(mut writer: impl Write, request: &Request) -> io::Result<()> {
    let line = match request {
        Request::Hello { version, token } => format!("HELLO {version} {token}\n"),
        Request::Ping { request_id } => format!("PING {request_id}\n"),
        Request::Shutdown => "SHUTDOWN\n".to_owned(),
    };
    writer.write_all(line.as_bytes())?;
    writer.flush()
}

pub fn read_request(reader: &mut impl BufRead) -> io::Result<Request> {
    parse_request(&read_control_line(reader)?)
}

pub fn write_response(mut writer: impl Write, response: &Response) -> io::Result<()> {
    let line = match response {
        Response::Hello { version } => format!("HELLO {version}\n"),
        Response::Pong { request_id } => format!("PONG {request_id}\n"),
        Response::Bye => "BYE\n".to_owned(),
        Response::Error(message) => format!("ERROR {}\n", message.replace(['\r', '\n'], " ")),
    };
    writer.write_all(line.as_bytes())?;
    writer.flush()
}

pub fn read_response(reader: &mut impl BufRead) -> io::Result<Response> {
    parse_response(&read_control_line(reader)?)
}

pub fn serve(stream: impl Read + Write, expected_token: &str) -> io::Result<()> {
    let mut stream = BufReader::new(stream);
    match read_request(&mut stream)? {
        Request::Hello { version, token }
            if version == PROTOCOL_VERSION && token == expected_token =>
        {
            write_response(
                stream.get_mut(),
                &Response::Hello {
                    version: PROTOCOL_VERSION,
                },
            )?;
        }
        _ => {
            write_response(
                stream.get_mut(),
                &Response::Error("incompatible hello".into()),
            )?;
            return Ok(());
        }
    }

    loop {
        match read_request(&mut stream)? {
            Request::Ping { request_id } => {
                write_response(stream.get_mut(), &Response::Pong { request_id })?;
            }
            Request::Shutdown => {
                write_response(stream.get_mut(), &Response::Bye)?;
                return Ok(());
            }
            Request::Hello { .. } => {
                write_response(stream.get_mut(), &Response::Error("duplicate hello".into()))?;
            }
        }
    }
}

pub fn run_client(stream: impl Read + Write, token: &str) -> io::Result<()> {
    let mut stream = BufReader::new(stream);
    write_request(
        stream.get_mut(),
        &Request::Hello {
            version: PROTOCOL_VERSION,
            token: token.to_owned(),
        },
    )?;
    expect_response(
        read_response(&mut stream)?,
        Response::Hello {
            version: PROTOCOL_VERSION,
        },
    )?;

    write_request(stream.get_mut(), &Request::Ping { request_id: 7 })?;
    expect_response(
        read_response(&mut stream)?,
        Response::Pong { request_id: 7 },
    )?;

    write_request(stream.get_mut(), &Request::Shutdown)?;
    expect_response(read_response(&mut stream)?, Response::Bye)
}

fn expect_response(actual: Response, expected: Response) -> io::Result<()> {
    if actual == expected {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("expected {expected:?}, received {actual:?}"),
        ))
    }
}

fn read_control_line(reader: &mut impl BufRead) -> io::Result<String> {
    let mut bytes = Vec::new();
    let mut limited = std::io::Read::take(&mut *reader, (MAX_CONTROL_LINE_BYTES + 1) as u64);
    let read = BufRead::read_until(&mut limited, b'\n', &mut bytes)?;
    if read == 0 {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "control channel closed",
        ));
    }
    if bytes.len() > MAX_CONTROL_LINE_BYTES || !bytes.ends_with(b"\n") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "control message exceeds the bounded line envelope",
        ));
    }
    String::from_utf8(bytes)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "control message is not UTF-8"))
}

fn parse_request(line: &str) -> io::Result<Request> {
    let mut fields = line.trim_end().split(' ');
    let request = match (fields.next(), fields.next(), fields.next(), fields.next()) {
        (Some("HELLO"), Some(version), Some(token), None) => Request::Hello {
            version: parse_number(version)?,
            token: token.to_owned(),
        },
        (Some("PING"), Some(request_id), None, None) => Request::Ping {
            request_id: parse_number(request_id)?,
        },
        (Some("SHUTDOWN"), None, None, None) => Request::Shutdown,
        _ => return Err(invalid_message("request")),
    };
    Ok(request)
}

fn parse_response(line: &str) -> io::Result<Response> {
    let line = line.trim_end();
    let mut fields = line.split(' ');
    let response = match (fields.next(), fields.next(), fields.next()) {
        (Some("HELLO"), Some(version), None) => Response::Hello {
            version: parse_number(version)?,
        },
        (Some("PONG"), Some(request_id), None) => Response::Pong {
            request_id: parse_number(request_id)?,
        },
        (Some("BYE"), None, None) => Response::Bye,
        (Some("ERROR"), _, _) => {
            Response::Error(line.strip_prefix("ERROR ").unwrap_or_default().to_owned())
        }
        _ => return Err(invalid_message("response")),
    };
    Ok(response)
}

fn parse_number<T: std::str::FromStr>(value: &str) -> io::Result<T> {
    value
        .parse()
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid numeric field"))
}

fn invalid_message(kind: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!("invalid control {kind}"),
    )
}

#[cfg(test)]
mod tests {
    use std::io::{BufReader, Cursor};

    use super::*;

    #[test]
    fn request_and_response_round_trip() {
        let mut bytes = Vec::new();
        write_request(
            &mut bytes,
            &Request::Hello {
                version: 1,
                token: "token".into(),
            },
        )
        .unwrap();
        assert_eq!(
            read_request(&mut BufReader::new(Cursor::new(bytes))).unwrap(),
            Request::Hello {
                version: 1,
                token: "token".into()
            }
        );
    }

    #[test]
    fn oversized_control_messages_fail_closed() {
        let bytes = vec![b'x'; MAX_CONTROL_LINE_BYTES + 1];
        assert_eq!(
            read_request(&mut BufReader::new(Cursor::new(bytes)))
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidData
        );
    }
}
