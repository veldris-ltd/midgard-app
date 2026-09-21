//! Minimal HTTP/1.1 request parsing + response writing over a std `TcpStream`. [NFR-SEC-1]
//! No external HTTP framework — just enough of HTTP/1.1 to serve a JSON API to
//! the browser consoles. Thread-per-connection (see `lib.rs`).

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;

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

/// Read and parse one request. Returns `None` on a cleanly closed connection.
pub fn read_request(stream: &TcpStream) -> std::io::Result<Option<Request>> {
    let mut reader = BufReader::new(stream);

    let mut request_line = String::new();
    let n = reader.read_line(&mut request_line)?;
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

    // Headers until a blank line: Content-Length, Authorization, Origin.
    let mut content_length = 0usize;
    let mut authorization = None;
    let mut origin = None;
    loop {
        let mut line = String::new();
        let n = reader.read_line(&mut line)?;
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
                content_length = value.parse().unwrap_or(0);
            } else if name.eq_ignore_ascii_case("authorization") {
                authorization = Some(value);
            } else if name.eq_ignore_ascii_case("origin") {
                origin = Some(value);
            }
        }
    }

    let mut body = String::new();
    if content_length > 0 {
        let mut buf = vec![0u8; content_length];
        reader.read_exact(&mut buf)?;
        body = String::from_utf8_lossy(&buf).into_owned();
    }

    Ok(Some(Request {
        method,
        path,
        query,
        body,
        authorization,
        origin,
    }))
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
}

impl Response {
    pub fn json(status: u16, body: String) -> Response {
        Response {
            status,
            content_type: "application/json; charset=utf-8",
            body,
            allow_origin: None,
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
        }
    }

    /// Set the allowed origin to echo (only call with an allowlisted origin).
    pub fn with_allow_origin(mut self, origin: Option<String>) -> Self {
        self.allow_origin = origin;
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
