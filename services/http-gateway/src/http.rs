//! Minimal HTTP/1.1 request parsing + response writing over a std `TcpStream`. [NFR-SEC-1]
//! No external HTTP framework — just enough of HTTP/1.1 to serve a JSON API to
//! the browser consoles. Thread-per-connection (see `lib.rs`).

use std::collections::BTreeSet;
use std::io::{BufRead, BufReader, ErrorKind, Read, Write};
use std::net::{IpAddr, Shutdown, SocketAddr, TcpStream};
use std::time::{Duration, Instant};

use service_runtime::ListenerLimits;

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
    /// The `X-Forwarded-For` header, its lines joined with `, ` in order, at most
    /// [`MAX_FORWARDED_FOR_BYTES`]. Believed only from a trusted proxy (see
    /// [`forwarded_client`]). [VLD-ANB-MIDGARD-0005]
    pub forwarded_for: Option<String>,
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
/// Largest `X-Forwarded-For` the edge reads, all its lines together: a chain of some
/// dozens of proxies. A longer one is refused. [VLD-ANB-MIDGARD-0005]
pub const MAX_FORWARDED_FOR_BYTES: usize = 1024;
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

/// [`read_request_within`] the edge's default head deadline,
/// [`ListenerLimits::edge`].
pub fn read_request(stream: &TcpStream) -> std::io::Result<Option<Incoming>> {
    read_request_within(stream, ListenerLimits::edge().head_deadline)
}

/// Read and parse one request, all of it — head and body — within `head_deadline`.
/// Returns `None` on a cleanly closed connection, and on one that has not sent its
/// request when the deadline passes, however it spaces its bytes: the read timeout is
/// set to the time remaining before every read from the socket. A read timeout already
/// on the socket still bounds each read, so a client silent for that long is also
/// `None`.
///
/// The head is capped at [`MAX_HEAD_BYTES`] and the body at [`MAX_BODY_BYTES`], so a
/// client cannot make the edge allocate what it declares; a chunked body, which
/// carries no length up front, is refused rather than silently read as empty.
/// [NFR-SEC-1] [VLD-ANB-MIDGARD-0004]
pub fn read_request_within(
    stream: &TcpStream,
    head_deadline: Duration,
) -> std::io::Result<Option<Incoming>> {
    let reader = DeadlineReader {
        stream,
        deadline: Instant::now() + head_deadline,
        idle: stream.read_timeout()?,
    };
    match read_request_by(reader) {
        Err(e) if matches!(e.kind(), ErrorKind::TimedOut | ErrorKind::WouldBlock) => Ok(None),
        other => other,
    }
}

/// Socket reads that never wait past `deadline`, nor longer than `idle` each.
struct DeadlineReader<'a> {
    stream: &'a TcpStream,
    deadline: Instant,
    idle: Option<Duration>,
}

impl Read for DeadlineReader<'_> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let left = self
            .deadline
            .checked_duration_since(Instant::now())
            .filter(|left| !left.is_zero())
            .ok_or_else(|| std::io::Error::from(ErrorKind::TimedOut))?;
        let wait = self.idle.map_or(left, |idle| idle.min(left));
        self.stream.set_read_timeout(Some(wait))?;
        let mut stream = self.stream;
        stream.read(buf)
    }
}

fn read_request_by(reader: DeadlineReader<'_>) -> std::io::Result<Option<Incoming>> {
    let mut reader = BufReader::new(reader);
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
    // Origin, X-Forwarded-For.
    let mut content_length = 0usize;
    let mut authorization = None;
    let mut origin = None;
    let mut forwarded_for: Option<String> = None;
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
            } else if name.eq_ignore_ascii_case("x-forwarded-for") {
                // Repeated lines are one list, in order (RFC 9110 §5.3).
                let joined = match forwarded_for.take() {
                    Some(before) => format!("{before}, {value}"),
                    None => value,
                };
                if joined.len() > MAX_FORWARDED_FOR_BYTES {
                    fault = Some((
                        400,
                        format!("X-Forwarded-For over {MAX_FORWARDED_FOR_BYTES} bytes"),
                    ));
                } else {
                    forwarded_for = Some(joined);
                }
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
        forwarded_for,
    })))
}

/// The client a request through `proxies` came from: the right-most address in
/// `forwarded_for` that is not itself one of `proxies`, each hop having appended the
/// address it was reached from. An entry may carry a port. `None` if there is no such
/// address, or the first entry that is not a trusted proxy is not an address: what is
/// left of it was written by the client, so it is not believed. The caller asks only
/// when the socket's peer is a trusted proxy. [VLD-ANB-MIDGARD-0005]
pub fn forwarded_client(forwarded_for: Option<&str>, proxies: &BTreeSet<IpAddr>) -> Option<IpAddr> {
    for entry in forwarded_for?.rsplit(',') {
        let entry = entry.trim();
        let ip = entry
            .parse::<IpAddr>()
            .or_else(|_| entry.parse::<SocketAddr>().map(|s| s.ip()))
            .ok()?
            .to_canonical();
        if !proxies.contains(&ip) {
            return Some(ip);
        }
    }
    None
}

/// Read one head line within what is left of the head's budget. `Some(0)` is a
/// closed connection; `None` means the head ran past [`MAX_HEAD_BYTES`].
fn read_head_line(
    reader: &mut impl BufRead,
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

/// Answer `response` on the accepting thread and close, without waiting on the client:
/// how a connection the edge will not serve at all — over the connection cap, or from a
/// source opening connections too fast — is refused. What the client has already sent
/// is read and discarded first, so it reads the refusal rather than a reset.
/// [VLD-ANB-MIDGARD-0004]
pub fn refuse_at_once(stream: &TcpStream, response: &Response) {
    let mut writer = stream;
    let _ = stream.set_write_timeout(Some(Duration::from_secs(1)));
    let _ = response.write(&mut writer);
    let _ = stream.shutdown(Shutdown::Write);
    if stream.set_nonblocking(true).is_ok() {
        let mut sink = [0u8; 4096];
        for _ in 0..(MAX_HEAD_BYTES / sink.len()) {
            match writer.read(&mut sink) {
                Ok(n) if n > 0 => {}
                _ => break,
            }
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
    /// Further response headers a route sets, sent as given and exposed to an
    /// allowlisted origin.
    pub headers: Vec<(&'static str, String)>,
    /// What went wrong, for the gateway's own log only: it is never written to the
    /// client. [VLD-ANB-MIDGARD-0053]
    pub detail: Option<String>,
}

impl Response {
    pub fn json(status: u16, body: String) -> Response {
        Response {
            status,
            content_type: "application/json; charset=utf-8",
            body,
            allow_origin: None,
            retry_after: None,
            headers: Vec::new(),
            detail: None,
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
            headers: Vec::new(),
            detail: None,
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

    /// Send header `name` with `value`. Only call with a fixed name and a value the
    /// edge itself produced, never one taken from the request.
    pub fn with_header(mut self, name: &'static str, value: &str) -> Self {
        self.headers.push((name, value.to_string()));
        self
    }

    /// A server error. The client reads only `{"error":"internal error"}` and the
    /// request's reference, which the gateway adds; `detail` goes to its log under
    /// the same reference. [VLD-ANB-MIDGARD-0053]
    pub fn internal(detail: impl Into<String>) -> Response {
        Response::json(500, r#"{"error":"internal error"}"#.to_string()).with_detail(detail)
    }

    /// Keep `detail` for the gateway's log; the client never reads it.
    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
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
            503 => "Service Unavailable",
            _ => "OK",
        }
    }

    pub fn write(&self, stream: &mut impl Write) -> std::io::Result<()> {
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
        for (name, value) in &self.headers {
            head.push_str(&format!("{name}: {value}\r\n"));
        }
        // Origin-locked CORS: echo only an allowlisted origin, never `*`.
        if let Some(origin) = &self.allow_origin {
            head.push_str(&format!("Access-Control-Allow-Origin: {origin}\r\n"));
            head.push_str("Vary: Origin\r\n");
            head.push_str("Access-Control-Allow-Methods: GET, POST, OPTIONS\r\n");
            head.push_str("Access-Control-Allow-Headers: Content-Type, Authorization\r\n");
            if !self.headers.is_empty() {
                let names: Vec<&str> = self.headers.iter().map(|(name, _)| *name).collect();
                let names = names.join(", ");
                head.push_str(&format!("Access-Control-Expose-Headers: {names}\r\n"));
            }
        }
        head.push_str("\r\n");
        stream.write_all(head.as_bytes())?;
        stream.write_all(body)?;
        stream.flush()
    }
}
