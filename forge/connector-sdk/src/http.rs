//! A minimal HTTP/1.1 client for network connectors: one request per connection, a
//! bounded response read with `Content-Length`, chunked transfer encoding or
//! connection close. No redirects are followed and nothing is cached. TLS is the egress
//! broker's job. [FR-F-2] [NFR-PRV-1]

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;

use crate::error::ConnectorError;

/// The largest response body read.
pub const MAX_BODY: usize = 256 * 1024 * 1024;
/// The largest status line or header read.
const MAX_LINE: usize = 16 * 1024;

/// A response: status, headers (names lowercase) and body.
#[derive(Debug)]
pub struct Response {
    /// The status code.
    pub status: u16,
    /// Headers, with lowercase names.
    pub headers: Vec<(String, String)>,
    /// The body.
    pub body: Vec<u8>,
}

impl Response {
    /// A header's value.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }
}

/// Send `GET target` over `stream` with `headers` (which must include `host`), and read
/// the response.
pub fn get(
    stream: TcpStream,
    target: &str,
    headers: &[(String, String)],
) -> Result<Response, ConnectorError> {
    let io = |e: std::io::Error| ConnectorError::Io(format!("HTTP: {e}"));
    let mut request = format!("GET {target} HTTP/1.1\r\n");
    for (name, value) in headers {
        if value.contains(['\r', '\n']) {
            return Err(ConnectorError::Parse(format!(
                "header {name} holds a line break"
            )));
        }
        request.push_str(&format!("{name}: {value}\r\n"));
    }
    request.push_str("Connection: close\r\n\r\n");
    let mut writer = stream.try_clone().map_err(io)?;
    writer.write_all(request.as_bytes()).map_err(io)?;

    let mut reader = BufReader::new(stream);
    let status_line = read_line(&mut reader)?;
    let status = status_line
        .strip_prefix("HTTP/1.")
        .and_then(|rest| rest.get(2..5))
        .and_then(|code| code.parse().ok())
        .ok_or_else(|| ConnectorError::Parse(format!("not an HTTP response: {status_line}")))?;

    let mut headers = Vec::new();
    loop {
        let line = read_line(&mut reader)?;
        if line.is_empty() {
            break;
        }
        let (name, value) = line
            .split_once(':')
            .ok_or_else(|| ConnectorError::Parse(format!("a malformed header: {line}")))?;
        headers.push((name.trim().to_ascii_lowercase(), value.trim().to_string()));
    }
    let find = |name: &str| {
        headers
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.clone())
    };

    let body = if find("transfer-encoding").is_some_and(|v| v.eq_ignore_ascii_case("chunked")) {
        let mut body = Vec::new();
        loop {
            let size_line = read_line(&mut reader)?;
            let size = usize::from_str_radix(size_line.split(';').next().unwrap_or("").trim(), 16)
                .map_err(|_| {
                    ConnectorError::Parse(format!("a malformed chunk size: {size_line}"))
                })?;
            if size == 0 {
                while !read_line(&mut reader)?.is_empty() {}
                break;
            }
            if body.len() + size > MAX_BODY {
                return Err(ConnectorError::Parse(
                    "the response body is too large".into(),
                ));
            }
            let start = body.len();
            body.resize(start + size, 0);
            reader.read_exact(&mut body[start..]).map_err(io)?;
            read_line(&mut reader)?;
        }
        body
    } else if let Some(length) = find("content-length") {
        let length: usize = length
            .parse()
            .ok()
            .filter(|&l| l <= MAX_BODY)
            .ok_or_else(|| ConnectorError::Parse(format!("an unusable content length {length}")))?;
        let mut body = vec![0u8; length];
        reader.read_exact(&mut body).map_err(io)?;
        body
    } else {
        let mut body = Vec::new();
        reader
            .take(MAX_BODY as u64 + 1)
            .read_to_end(&mut body)
            .map_err(io)?;
        if body.len() > MAX_BODY {
            return Err(ConnectorError::Parse(
                "the response body is too large".into(),
            ));
        }
        body
    };
    Ok(Response {
        status,
        headers,
        body,
    })
}

fn read_line(reader: &mut BufReader<TcpStream>) -> Result<String, ConnectorError> {
    let mut line = Vec::new();
    let read = reader
        .by_ref()
        .take(MAX_LINE as u64)
        .read_until(b'\n', &mut line)
        .map_err(|e| ConnectorError::Io(format!("HTTP: {e}")))?;
    if read == 0 {
        return Err(ConnectorError::Io(
            "HTTP: the connection closed early".into(),
        ));
    }
    if line.last() != Some(&b'\n') {
        return Err(ConnectorError::Parse("an HTTP line is too long".into()));
    }
    line.pop();
    if line.last() == Some(&b'\r') {
        line.pop();
    }
    String::from_utf8(line).map_err(|_| ConnectorError::Parse("an HTTP line is not UTF-8".into()))
}
