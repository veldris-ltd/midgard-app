//! Minimal HTTP/1.1 request parsing + response writing over a std `TcpStream`. [NFR-SEC-1]
//! No external HTTP framework — just enough of HTTP/1.1 to serve a JSON API to
//! the browser consoles. Thread-per-connection (see `lib.rs`).

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Shutdown, TcpStream};
use std::time::{Duration, Instant};

/// A parsed HTTP request.
pub struct Request {
    pub method: String,
    /// Path without the query string, e.g. `/api/v1/query`.
    pub path: String,
    /// Raw query string (without `?`), e.g. `limit=40`.
    pub query: String,
    pub body: String,
    /// The `Authorization` header value (the PQ-signed IdP bearer token), if any.
    pub authorization: Option<String>,
    /// The browser `Origin` header, for origin-locked CORS.
    pub origin: Option<String>,
}

impl Request {
    /// The bearer token from `Authorization: Bearer <token>`, if present.
    pub fn bearer(&self) -> Option<&str> {
        self.authorization
            .as_deref()
            .and_then(|a| a.strip_prefix("Bearer "))
            .map(str::trim)
    }
}

impl Request {
    /// Value of a single query parameter, if present.
    pub fn query_param(&self, key: &str) -> Option<String> {
        for pair in self.query.split('&') {
            let mut it = pair.splitn(2, '=');
            if it.next() == Some(key) {
                return Some(it.next().unwrap_or("").to_string());
            }
        }
        None
    }
}

/// Largest request body the edge reads. Console and administration bodies are a few
/// KiB; anything near this is a mistake or an attack, refused before it is read.
pub const MAX_BODY_BYTES: usize = 1024 * 1024;
/// Largest request line plus headers the edge reads.
pub const MAX_HEAD_BYTES: usize = 16 * 1024;
/// How long a refused connection is drained, so the client reads the refusal rather
/// than a reset, before it is closed.
const LINGER: Duration = Duration::from_secs(2);

/// One request off the wire: either parsed, or refused before its body is read.
pub enum Incoming {
    /// A request to serve.
    Request(Request),
    /// A request the edge will not read — too large, or framed in a way it does not
    /// accept. Answer with `response`, then [`linger`] and close. `origin` is the
    /// request's `Origin`, when its head was read far enough to have one, so an
    /// allowlisted console can still read the refusal.
    Refused {
        response: Response,
        origin: Option<String>,
    },
}

/// Read and parse one request. Returns `None` on a cleanly closed connection.
///
/// The head is capped at [`MAX_HEAD_BYTES`] and the body at [`MAX_BODY_BYTES`], so a
/// client cannot make the edge allocate what it declares; a chunked body, which
/// carries no length up front, is refused rather than silently read as empty.
/// [NFR-SEC-1]
pub fn read_request(stream: &TcpStream) -> std::io::Result<Option<Incoming>> {
    let mut reader = BufReader::new(stream);
    let mut head_left = MAX_HEAD_BYTES;

    let mut request_line = String::new();
    let n = match read_head_line(&mut reader, &mut request_line, &mut head_left)? {
        Some(n) => n,
        None => return Ok(Some(head_too_large(None))),
    };
    if n == 0 {
        return Ok(None); // connection closed
    }
    // `split_whitespace` already discards the trailing CRLF, so no `trim_end`.
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let target = parts.next().unwrap_or("/").to_string();

    let (path, query) = match target.split_once('?') {
        Some((p, q)) => (p.to_string(), q.to_string()),
        None => (target, String::new()),
    };

    // Headers until a blank line: Content-Length, Transfer-Encoding, Authorization,
    // Origin.
    let mut content_length = 0usize;
    let mut authorization = None;
    let mut origin = None;
    // A framing fault is answered once the whole head is read, so the refusal can
    // carry the request's origin.
    let mut fault: Option<(u16, String)> = None;
    loop {
        let mut line = String::new();
        let n = match read_head_line(&mut reader, &mut line, &mut head_left)? {
            Some(n) => n,
            None => return Ok(Some(head_too_large(origin))),
        };
        if n == 0 {
            break;
        }
        let line = line.trim_end();
        if line.is_empty() {
            break;
        }
        if let Some((name, value)) = line.split_once(':') {
            let name = name.trim();
            let value = value.trim().to_string();
            if name.eq_ignore_ascii_case("content-length") {
                match value.parse() {
                    Ok(n) => content_length = n,
                    Err(_) => fault = Some((400, "malformed Content-Length".into())),
                }
            } else if name.eq_ignore_ascii_case("transfer-encoding") {
                fault = Some((
                    411,
                    "chunked bodies are not accepted: send Content-Length".into(),
                ));
            } else if name.eq_ignore_ascii_case("authorization") {
                authorization = Some(value);
            } else if name.eq_ignore_ascii_case("origin") {
                origin = Some(value);
            }
        }
    }

    if fault.is_none() && content_length > MAX_BODY_BYTES {
        fault = Some((413, format!("request body over {MAX_BODY_BYTES} bytes")));
    }
    if let Some((status, message)) = fault {
        return Ok(Some(refuse(status, &message, origin)));
    }
    let mut body = String::new();
    if content_length > 0 {
        let mut buf = vec![0u8; content_length];
        reader.read_exact(&mut buf)?;
        body = String::from_utf8_lossy(&buf).into_owned();
    }

    Ok(Some(Incoming::Request(Request {
        method,
        path,
        query,
        body,
        authorization,
        origin,
    })))
}

/// Read one head line within what is left of the head's budget. `Some(0)` is a
/// closed connection; `None` means the head ran past [`MAX_HEAD_BYTES`].
fn read_head_line(
    reader: &mut BufReader<&TcpStream>,
    line: &mut String,
    left: &mut usize,
) -> std::io::Result<Option<usize>> {
    if *left == 0 {
        return Ok(None);
    }
    let budget = u64::try_from(*left).unwrap_or(u64::MAX);
    let n = reader.by_ref().take(budget).read_line(line)?;
    if n > 0 && !line.ends_with('\n') && n >= *left {
        return Ok(None);
    }
    *left -= n;
    Ok(Some(n))
}

fn head_too_large(origin: Option<String>) -> Incoming {
    refuse(
        431,
        &format!("request head over {MAX_HEAD_BYTES} bytes"),
        origin,
    )
}

fn refuse(status: u16, message: &str, origin: Option<String>) -> Incoming {
    let body = format!(r#"{{"error":"{}"}}"#, message.replace('"', "'"));
    Incoming::Refused {
        response: Response::json(status, body),
        origin,
    }
}

/// After answering a refused request, stop writing and drain what the client is still
/// sending — for at most [`LINGER`] — so it reads the refusal instead of a connection
/// reset. Nothing drained is kept.
pub fn linger(stream: &TcpStream) {
    let _ = stream.shutdown(Shutdown::Write);
    let deadline = Instant::now() + LINGER;
    let mut sink = [0u8; 16 * 1024];
    let mut reader = stream;
    while let Some(left) = deadline.checked_duration_since(Instant::now()) {
        if left.is_zero() || stream.set_read_timeout(Some(left)).is_err() {
            break;
        }
        match reader.read(&mut sink) {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
    }
}

/// An HTTP response with a JSON (or text) body and **origin-locked** CORS: the
/// `Access-Control-Allow-Origin` header is only emitted when the request's origin
/// is on the configured allowlist (`allow_origin` is `Some`) — never `*`.
pub struct Response {
    pub status: u16,
    pub content_type: &'static str,
    pub body: String,
    /// The exact origin to allow (echoed), or `None` to send no CORS grant.
    pub allow_origin: Option<String>,
    /// Seconds before the client should retry, sent as `Retry-After`.
    pub retry_after: Option<u64>,
}

impl Response {
    pub fn json(status: u16, body: String) -> Response {
        Response {
            status,
            content_type: "application/json; charset=utf-8",
            body,
            allow_origin: None,
            retry_after: None,
        }
    }

    pub fn ok(body: String) -> Response {
        Response::json(200, body)
    }

    pub fn text(status: u16, body: &str) -> Response {
        Response {
            status,
            content_type: "text/plain; charset=utf-8",
            body: body.to_string(),
            allow_origin: None,
            retry_after: None,
        }
    }

    /// Set the allowed origin to echo (only call with an allowlisted origin).
    pub fn with_allow_origin(mut self, origin: Option<String>) -> Self {
        self.allow_origin = origin;
        self
    }

    /// Ask the client to wait `seconds` before retrying.
    pub fn with_retry_after(mut self, seconds: u64) -> Self {
        self.retry_after = Some(seconds);
        self
    }

    fn reason(&self) -> &'static str {
        match self.status {
            200 => "OK",
            204 => "No Content",
            400 => "Bad Request",
            401 => "Unauthorized",
            403 => "Forbidden",
            404 => "Not Found",
            405 => "Method Not Allowed",
            411 => "Length Required",
            413 => "Payload Too Large",
            429 => "Too Many Requests",
            431 => "Request Header Fields Too Large",
            500 => "Internal Server Error",
            _ => "OK",
        }
    }

    pub fn write(&self, stream: &mut TcpStream) -> std::io::Result<()> {
        let body = self.body.as_bytes();
        let mut head = format!(
            "HTTP/1.1 {} {}\r\n\
             Content-Type: {}\r\n\
             Content-Length: {}\r\n\
             Cache-Control: no-store\r\n\
             Connection: close\r\n",
            self.status,
            self.reason(),
            self.content_type,
            body.len(),
        );
        if self.status == 401 {
            head.push_str("WWW-Authenticate: Bearer\r\n");
        }
        if let Some(seconds) = self.retry_after {
            head.push_str(&format!("Retry-After: {seconds}\r\n"));
        }
        // Origin-locked CORS: echo only an allowlisted origin, never `*`.
        if let Some(origin) = &self.allow_origin {
            head.push_str(&format!("Access-Control-Allow-Origin: {origin}\r\n"));
            head.push_str("Vary: Origin\r\n");
            head.push_str("Access-Control-Allow-Methods: GET, POST, OPTIONS\r\n");
            head.push_str("Access-Control-Allow-Headers: Content-Type, Authorization\r\n");
        }
        head.push_str("\r\n");
        stream.write_all(head.as_bytes())?;
        stream.write_all(body)?;
        stream.flush()
    }
}
