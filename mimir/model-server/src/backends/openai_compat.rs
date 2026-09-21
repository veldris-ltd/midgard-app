//! The serving-sidecar client: OpenAI-compatible completions, streamed over loopback.
//! [FR-MI-1] [DD-4] [NFR-PERF-1]
//!
//! vLLM (GPU) and llama.cpp's `llama-server` (CPU) both serve `POST /v1/completions`
//! with `"stream": true`, answering with server-sent events: one `data: {json}` per
//! generated piece, then `data: [DONE]`. One client therefore drives either engine.
//!
//! The sidecar runs beside the model plane, in its pod or its container's network
//! namespace, and is reached **only on a loopback address**. A non-loopback address is
//! refused when the backend is built, so the model plane never reaches across a network
//! for a completion, and the no-HTTP-between-services rule holds.
//!
//! The client is hand-rolled on std's `TcpStream`: HTTP/1.1 with `Connection: close`,
//! chunked, length-delimited or close-delimited bodies, and bounded lines and responses.
//! First-token latency is measured from sending the request to receiving the first
//! non-empty piece. Any failure fails the call closed: an unreachable sidecar, a non-200
//! status, a malformed event, or a stream that ends before `[DONE]`. Nothing falls back
//! to another engine.

use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::{Duration, Instant};

use crate::adapter::FamilyAdapter;
use crate::backend::{
    BackendDescriptor, InferError, Locality, ModelBackend, PreparedRequest, RawCompletion,
};
use crate::hardware::HwProfile;
use crate::quant::Quant;

/// The longest status or header line accepted.
const MAX_HEAD_LINE: u64 = 8 * 1024;
/// The most headers accepted.
const MAX_HEADERS: usize = 100;
/// The longest event line accepted.
const MAX_EVENT_LINE: u64 = 1024 * 1024;
/// The most event bytes one completion may stream.
const MAX_RESPONSE: usize = 16 * 1024 * 1024;

/// The serving engine behind the sidecar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Engine {
    /// vLLM, on GPUs.
    Vllm,
    /// llama.cpp's `llama-server`, on CPUs (GGUF weights).
    LlamaCpp,
}

impl Engine {
    /// The engine's name.
    pub fn as_str(self) -> &'static str {
        match self {
            Engine::Vllm => "vllm",
            Engine::LlamaCpp => "llama.cpp",
        }
    }
}

/// A model served by an OpenAI-compatible sidecar on loopback.
pub struct SidecarBackend {
    descriptor: BackendDescriptor,
    engine: Engine,
    addr: SocketAddr,
    model: String,
    adapter: Option<Box<dyn FamilyAdapter>>,
    connect_timeout: Duration,
    read_timeout: Duration,
}

impl SidecarBackend {
    /// A backend for the sidecar on `addr`, serving `model` (the name the sidecar
    /// serves it under). Refused unless `addr` is a loopback address and the engine is
    /// in-enclosure. [DD-4]
    pub fn new(
        descriptor: BackendDescriptor,
        engine: Engine,
        addr: SocketAddr,
        model: impl Into<String>,
    ) -> Result<Self, InferError> {
        if !addr.ip().is_loopback() {
            return Err(InferError::Unavailable(format!(
                "the {} sidecar must be on a loopback address, not {addr}",
                engine.as_str()
            )));
        }
        if descriptor.locality != Locality::InEnclosure {
            return Err(InferError::Unavailable(format!(
                "engine '{}' is not in-enclosure",
                descriptor.id
            )));
        }
        Ok(Self {
            descriptor,
            engine,
            addr,
            model: model.into(),
            adapter: None,
            connect_timeout: Duration::from_millis(500),
            read_timeout: Duration::from_secs(120),
        })
    }

    /// A llama.cpp engine serving one `family` model from GGUF weights on the CPU. It
    /// ranks above the reference engine, so a node that runs it serves real text.
    pub fn llama_cpp(
        id: impl Into<String>,
        addr: SocketAddr,
        model: impl Into<String>,
        family: &str,
    ) -> Result<Self, InferError> {
        Self::new(
            BackendDescriptor {
                id: id.into(),
                locality: Locality::InEnclosure,
                hw: HwProfile::none(),
                quants: vec![Quant::Gguf],
                rank: 15,
                families: vec![family.to_string()],
            },
            Engine::LlamaCpp,
            addr,
            model,
        )
    }

    /// A vLLM engine needing `hw`, serving `families`.
    pub fn vllm(
        id: impl Into<String>,
        addr: SocketAddr,
        model: impl Into<String>,
        families: Vec<String>,
        hw: HwProfile,
        rank: u8,
    ) -> Result<Self, InferError> {
        Self::new(
            BackendDescriptor {
                id: id.into(),
                locality: Locality::InEnclosure,
                hw,
                // GPU engines prefer quantised weights for latency and footprint.
                quants: vec![Quant::Awq, Quant::Int4, Quant::Int8, Quant::None],
                rank,
                families,
            },
            Engine::Vllm,
            addr,
            model,
        )
    }

    /// Wrap each prompt in `adapter`'s chat template, stop at its stop tokens, and
    /// trim them from the completion. [FR-MS-7]
    pub fn with_adapter(mut self, adapter: Box<dyn FamilyAdapter>) -> Self {
        self.adapter = Some(adapter);
        self
    }

    /// Override the connect timeout and the per-read timeout.
    pub fn with_timeouts(mut self, connect: Duration, read: Duration) -> Self {
        self.connect_timeout = connect;
        self.read_timeout = read;
        self
    }

    /// The engine.
    pub fn engine(&self) -> Engine {
        self.engine
    }

    /// The sidecar's loopback address.
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// Whether the sidecar reports itself ready: `GET /health` answers 200. vLLM and
    /// `llama-server` both serve it, and `llama-server` answers 503 while it is still
    /// loading the model.
    pub fn healthy(&self) -> bool {
        let probe = || -> Result<u16, String> {
            let mut stream = TcpStream::connect_timeout(&self.addr, Duration::from_millis(300))
                .map_err(|e| e.to_string())?;
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .and_then(|()| stream.set_write_timeout(Some(Duration::from_secs(2))))
                .map_err(|e| e.to_string())?;
            write!(
                stream,
                "GET /health HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\r\n",
                self.addr
            )
            .map_err(|e| e.to_string())?;
            read_head(&mut BufReader::new(stream)).map(|head| head.status)
        };
        probe() == Ok(200)
    }

    /// Stream one completion from the sidecar.
    fn complete(&self, req: &PreparedRequest) -> Result<RawCompletion, String> {
        let prompt = match &self.adapter {
            Some(adapter) => adapter.render(&req.prompt),
            None => req.prompt.clone(),
        };
        let mut body = format!(
            "{{\"model\":{},\"prompt\":{},\"max_tokens\":{},\"stream\":true",
            json::string(&self.model),
            json::string(&prompt),
            req.max_tokens.max(1)
        );
        if let Some(adapter) = &self.adapter {
            let stops: Vec<String> = adapter
                .stop_tokens()
                .iter()
                .map(|s| json::string(s))
                .collect();
            body.push_str(&format!(",\"stop\":[{}]", stops.join(",")));
        }
        if self.engine == Engine::Vllm {
            // vLLM reports token counts in a final event only when asked.
            body.push_str(",\"stream_options\":{\"include_usage\":true}");
        }
        body.push('}');

        let mut stream = TcpStream::connect_timeout(&self.addr, self.connect_timeout)
            .map_err(|e| format!("unreachable: {e}"))?;
        stream
            .set_read_timeout(Some(self.read_timeout))
            .and_then(|()| stream.set_write_timeout(Some(self.read_timeout)))
            .map_err(|e| e.to_string())?;
        let head = format!(
            "POST /v1/completions HTTP/1.1\r\nHost: {}\r\nContent-Type: application/json\r\n\
             Accept: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            self.addr,
            body.len()
        );
        stream
            .write_all(head.as_bytes())
            .and_then(|()| stream.write_all(body.as_bytes()))
            .map_err(|e| format!("cannot send the request: {e}"))?;
        let start = Instant::now();

        let mut reader = BufReader::new(stream);
        let head = read_head(&mut reader)?;
        let body: Box<dyn Read> = if head.chunked {
            Box::new(Chunked::new(reader))
        } else if let Some(length) = head.content_length {
            Box::new(reader.take(length))
        } else {
            Box::new(reader)
        };
        let mut events = BufReader::new(body);
        if head.status != 200 {
            let mut detail = Vec::new();
            let _ = events.take(512).read_to_end(&mut detail);
            let detail = String::from_utf8_lossy(&detail);
            let detail = detail.split_whitespace().collect::<Vec<_>>().join(" ");
            return Err(format!("answered HTTP {}: {detail}", head.status));
        }

        let mut text = String::new();
        let mut pieces = 0u32;
        let mut first_token_ms = None;
        let mut usage = None;
        let mut cache_hit = false;
        let mut streamed = 0usize;
        let mut done = false;
        while let Some(line) = read_event_line(&mut events)? {
            streamed += line.len();
            if streamed > MAX_RESPONSE {
                return Err("the response is too large".into());
            }
            // Blank lines end an event; `:` lines are comments; other fields (`event:`,
            // `id:`) carry nothing a completion needs.
            let Some(data) = line.strip_prefix("data:") else {
                continue;
            };
            let data = data.trim_start();
            if data == "[DONE]" {
                done = true;
                break;
            }
            let event = json::parse(data).map_err(|e| format!("malformed event: {e}"))?;
            if let Some(error) = event.get("error") {
                return Err(format!(
                    "the sidecar reported an error: {}",
                    error.summary()
                ));
            }
            let piece = event
                .get("choices")
                .and_then(|choices| choices.first())
                .and_then(|choice| choice.get("text"))
                .and_then(|text| text.as_str());
            if let Some(piece) = piece.filter(|p| !p.is_empty()) {
                if first_token_ms.is_none() {
                    first_token_ms = Some(start.elapsed().as_millis());
                }
                text.push_str(piece);
                pieces += 1;
            }
            if let Some(counts) = event.get("usage") {
                let count = |key: &str| counts.get(key).and_then(|n| n.as_u64());
                if let (Some(prompt), Some(completion)) =
                    (count("prompt_tokens"), count("completion_tokens"))
                {
                    usage = Some((prompt, completion));
                }
            }
            // llama-server reports how much of the prompt it served from its cache.
            if let Some(cached) = event
                .get("timings")
                .and_then(|t| t.get("cache_n"))
                .and_then(|n| n.as_u64())
            {
                cache_hit |= cached > 0;
            }
        }
        if !done {
            return Err("the stream ended before [DONE]".into());
        }

        let text = match &self.adapter {
            Some(adapter) => adapter.postprocess(&text),
            None => text,
        };
        let clamp = |n: u64| u32::try_from(n).unwrap_or(u32::MAX);
        let (prompt_tokens, completion_tokens) = match usage {
            Some((prompt, completion)) => (clamp(prompt), clamp(completion)),
            None => (req.prompt.split_whitespace().count().max(1) as u32, pieces),
        };
        Ok(RawCompletion {
            text,
            first_token_ms: clamp(
                first_token_ms.unwrap_or_else(|| start.elapsed().as_millis()) as u64,
            ),
            cache_hit,
            prompt_tokens,
            completion_tokens,
        })
    }
}

impl ModelBackend for SidecarBackend {
    fn descriptor(&self) -> &BackendDescriptor {
        &self.descriptor
    }

    fn infer_batch(&self, batch: &[PreparedRequest]) -> Result<Vec<RawCompletion>, InferError> {
        // One streamed call per request; the sidecar batches concurrent calls itself.
        batch
            .iter()
            .map(|req| {
                self.complete(req).map_err(|e| {
                    InferError::Unavailable(format!(
                        "{} sidecar for '{}' on {}: {e}",
                        self.engine.as_str(),
                        self.descriptor.id,
                        self.addr
                    ))
                })
            })
            .collect()
    }
}

/// The parts of a response head the client uses.
struct Head {
    status: u16,
    chunked: bool,
    content_length: Option<u64>,
}

fn read_head(r: &mut impl BufRead) -> Result<Head, String> {
    let status_line = read_line(r, MAX_HEAD_LINE).map_err(|e| format!("no response: {e}"))?;
    let mut parts = status_line.split_whitespace();
    if !parts.next().is_some_and(|v| v.starts_with("HTTP/1.")) {
        return Err(format!("not an HTTP/1 response: {status_line:?}"));
    }
    let status = parts
        .next()
        .and_then(|s| s.parse::<u16>().ok())
        .ok_or_else(|| format!("bad status line {status_line:?}"))?;
    let mut chunked = false;
    let mut content_length = None;
    for _ in 0..=MAX_HEADERS {
        let line = read_line(r, MAX_HEAD_LINE).map_err(|e| format!("truncated headers: {e}"))?;
        if line.is_empty() {
            return Ok(Head {
                status,
                chunked,
                content_length,
            });
        }
        let (name, value) = line
            .split_once(':')
            .ok_or_else(|| format!("malformed header {line:?}"))?;
        let value = value.trim();
        if name.eq_ignore_ascii_case("transfer-encoding") {
            chunked = value.to_ascii_lowercase().contains("chunked");
        } else if name.eq_ignore_ascii_case("content-length") {
            content_length = Some(
                value
                    .parse::<u64>()
                    .map_err(|_| format!("bad content-length {value:?}"))?,
            );
        }
    }
    Err("too many headers".into())
}

/// One line ending in LF (a preceding CR is dropped), refusing a line longer than `max`.
fn read_line(r: &mut impl BufRead, max: u64) -> io::Result<String> {
    let mut buf = Vec::new();
    r.by_ref().take(max + 1).read_until(b'\n', &mut buf)?;
    if buf.last() != Some(&b'\n') {
        return Err(if buf.len() as u64 > max {
            io::Error::new(io::ErrorKind::InvalidData, "line too long")
        } else {
            io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "the connection closed mid-line",
            )
        });
    }
    buf.pop();
    if buf.last() == Some(&b'\r') {
        buf.pop();
    }
    String::from_utf8(buf).map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "not UTF-8"))
}

/// The next event-stream line, or `None` at the end of the body. A last line without
/// its newline still counts.
fn read_event_line(r: &mut impl BufRead) -> Result<Option<String>, String> {
    let mut buf = Vec::new();
    r.by_ref()
        .take(MAX_EVENT_LINE + 1)
        .read_until(b'\n', &mut buf)
        .map_err(|e| format!("cannot read the stream: {e}"))?;
    if buf.is_empty() {
        return Ok(None);
    }
    if buf.last() == Some(&b'\n') {
        buf.pop();
    } else if buf.len() as u64 > MAX_EVENT_LINE {
        return Err("an event line is too long".into());
    }
    if buf.last() == Some(&b'\r') {
        buf.pop();
    }
    String::from_utf8(buf)
        .map(Some)
        .map_err(|_| "an event line is not UTF-8".into())
}

/// A chunked transfer-coded body, decoded.
struct Chunked<R> {
    inner: R,
    remaining: u64,
    finished: bool,
}

impl<R: BufRead> Chunked<R> {
    fn new(inner: R) -> Self {
        Self {
            inner,
            remaining: 0,
            finished: false,
        }
    }
}

impl<R: BufRead> Read for Chunked<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.finished || buf.is_empty() {
            return Ok(0);
        }
        if self.remaining == 0 {
            let line = read_line(&mut self.inner, MAX_HEAD_LINE)?;
            let size = line.split(';').next().unwrap_or("").trim();
            let size = u64::from_str_radix(size, 16)
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "bad chunk size"))?;
            if size == 0 {
                // Trailers, then the blank line that ends the body.
                while !read_line(&mut self.inner, MAX_HEAD_LINE)?.is_empty() {}
                self.finished = true;
                return Ok(0);
            }
            self.remaining = size;
        }
        let want = self.remaining.min(buf.len() as u64) as usize;
        let n = self.inner.read(&mut buf[..want])?;
        if n == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "the connection closed mid-chunk",
            ));
        }
        self.remaining -= n as u64;
        if self.remaining == 0 && !read_line(&mut self.inner, 2)?.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "a chunk is not terminated",
            ));
        }
        Ok(n)
    }
}

/// Just enough JSON for the completion events: a strict parser and a string encoder.
mod json {
    /// A JSON value.
    #[derive(Debug, Clone, PartialEq)]
    pub enum Json {
        Null,
        Bool(bool),
        Num(f64),
        Str(String),
        Arr(Vec<Json>),
        Obj(Vec<(String, Json)>),
    }

    impl Json {
        /// An object's member.
        pub fn get(&self, key: &str) -> Option<&Json> {
            match self {
                Json::Obj(members) => members.iter().find(|(k, _)| k == key).map(|(_, v)| v),
                _ => None,
            }
        }
        /// An array's first element.
        pub fn first(&self) -> Option<&Json> {
            match self {
                Json::Arr(items) => items.first(),
                _ => None,
            }
        }
        /// A string's value.
        pub fn as_str(&self) -> Option<&str> {
            match self {
                Json::Str(s) => Some(s),
                _ => None,
            }
        }
        /// A non-negative whole number.
        pub fn as_u64(&self) -> Option<u64> {
            match self {
                Json::Num(n) if *n >= 0.0 && n.fract() == 0.0 && *n <= 9.0e15 => Some(*n as u64),
                _ => None,
            }
        }
        /// A short, single-line rendering for an error message.
        pub fn summary(&self) -> String {
            let text = match self {
                Json::Str(s) => s.clone(),
                Json::Obj(_) => self
                    .get("message")
                    .and_then(Json::as_str)
                    .map_or_else(|| "(no message)".to_string(), str::to_string),
                other => format!("{other:?}"),
            };
            text.chars().filter(|c| !c.is_control()).take(200).collect()
        }
    }

    /// Encode `s` as a JSON string.
    pub fn string(s: &str) -> String {
        let mut out = String::with_capacity(s.len() + 2);
        out.push('"');
        for c in s.chars() {
            match c {
                '"' => out.push_str("\\\""),
                '\\' => out.push_str("\\\\"),
                '\n' => out.push_str("\\n"),
                '\r' => out.push_str("\\r"),
                '\t' => out.push_str("\\t"),
                c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
                c => out.push(c),
            }
        }
        out.push('"');
        out
    }

    /// Parse one JSON value, refusing trailing input and nesting deeper than 64.
    pub fn parse(input: &str) -> Result<Json, String> {
        let mut p = Parser {
            s: input,
            b: input.as_bytes(),
            i: 0,
            depth: 0,
        };
        let value = p.value()?;
        p.ws();
        if p.i != p.b.len() {
            return Err(format!("trailing input at byte {}", p.i));
        }
        Ok(value)
    }

    struct Parser<'a> {
        s: &'a str,
        b: &'a [u8],
        i: usize,
        depth: usize,
    }

    impl Parser<'_> {
        fn ws(&mut self) {
            while self.i < self.b.len() && matches!(self.b[self.i], b' ' | b'\t' | b'\n' | b'\r') {
                self.i += 1;
            }
        }

        fn eat(&mut self, byte: u8) -> Result<(), String> {
            self.ws();
            if self.b.get(self.i) == Some(&byte) {
                self.i += 1;
                Ok(())
            } else {
                Err(format!("expected '{}' at byte {}", byte as char, self.i))
            }
        }

        fn value(&mut self) -> Result<Json, String> {
            self.ws();
            match self.b.get(self.i) {
                Some(b'{') => self.nested(Self::object),
                Some(b'[') => self.nested(Self::array),
                Some(b'"') => self.string().map(Json::Str),
                Some(b't') => self.literal("true", Json::Bool(true)),
                Some(b'f') => self.literal("false", Json::Bool(false)),
                Some(b'n') => self.literal("null", Json::Null),
                Some(b'-' | b'0'..=b'9') => self.number(),
                Some(_) => Err(format!("unexpected byte at {}", self.i)),
                None => Err("unexpected end of input".into()),
            }
        }

        fn nested(&mut self, parse: fn(&mut Self) -> Result<Json, String>) -> Result<Json, String> {
            self.depth += 1;
            if self.depth > 64 {
                return Err("nested too deeply".into());
            }
            let value = parse(self);
            self.depth -= 1;
            value
        }

        fn object(&mut self) -> Result<Json, String> {
            self.eat(b'{')?;
            let mut members = Vec::new();
            self.ws();
            if self.b.get(self.i) == Some(&b'}') {
                self.i += 1;
                return Ok(Json::Obj(members));
            }
            loop {
                self.ws();
                let key = self.string()?;
                self.eat(b':')?;
                members.push((key, self.value()?));
                self.ws();
                match self.b.get(self.i) {
                    Some(b',') => self.i += 1,
                    Some(b'}') => {
                        self.i += 1;
                        return Ok(Json::Obj(members));
                    }
                    _ => return Err(format!("expected ',' or '}}' at byte {}", self.i)),
                }
            }
        }

        fn array(&mut self) -> Result<Json, String> {
            self.eat(b'[')?;
            let mut items = Vec::new();
            self.ws();
            if self.b.get(self.i) == Some(&b']') {
                self.i += 1;
                return Ok(Json::Arr(items));
            }
            loop {
                items.push(self.value()?);
                self.ws();
                match self.b.get(self.i) {
                    Some(b',') => self.i += 1,
                    Some(b']') => {
                        self.i += 1;
                        return Ok(Json::Arr(items));
                    }
                    _ => return Err(format!("expected ',' or ']' at byte {}", self.i)),
                }
            }
        }

        fn literal(&mut self, word: &str, value: Json) -> Result<Json, String> {
            if self.b[self.i..].starts_with(word.as_bytes()) {
                self.i += word.len();
                Ok(value)
            } else {
                Err(format!("bad literal at byte {}", self.i))
            }
        }

        fn number(&mut self) -> Result<Json, String> {
            let start = self.i;
            while self.i < self.b.len()
                && matches!(
                    self.b[self.i],
                    b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9'
                )
            {
                self.i += 1;
            }
            self.s[start..self.i]
                .parse::<f64>()
                .ok()
                .filter(|n| n.is_finite())
                .map(Json::Num)
                .ok_or_else(|| format!("bad number at byte {start}"))
        }

        fn hex4(&mut self) -> Result<u32, String> {
            let digits = self
                .s
                .get(self.i..self.i + 4)
                .ok_or("truncated \\u escape")?;
            let n = u32::from_str_radix(digits, 16).map_err(|_| "bad \\u escape")?;
            self.i += 4;
            Ok(n)
        }

        fn string(&mut self) -> Result<String, String> {
            if self.b.get(self.i) != Some(&b'"') {
                return Err(format!("expected a string at byte {}", self.i));
            }
            self.i += 1;
            let mut out = String::new();
            let mut run = self.i;
            loop {
                match self.b.get(self.i) {
                    None => return Err("unterminated string".into()),
                    Some(b'"') => {
                        out.push_str(&self.s[run..self.i]);
                        self.i += 1;
                        return Ok(out);
                    }
                    Some(b'\\') => {
                        out.push_str(&self.s[run..self.i]);
                        self.i += 1;
                        let escape = *self.b.get(self.i).ok_or("unterminated escape")?;
                        self.i += 1;
                        match escape {
                            b'"' => out.push('"'),
                            b'\\' => out.push('\\'),
                            b'/' => out.push('/'),
                            b'b' => out.push('\u{8}'),
                            b'f' => out.push('\u{c}'),
                            b'n' => out.push('\n'),
                            b'r' => out.push('\r'),
                            b't' => out.push('\t'),
                            b'u' => {
                                let mut code = self.hex4()?;
                                if (0xD800..0xDC00).contains(&code) {
                                    if !self.b[self.i..].starts_with(b"\\u") {
                                        return Err("unpaired surrogate".into());
                                    }
                                    self.i += 2;
                                    let low = self.hex4()?;
                                    if !(0xDC00..0xE000).contains(&low) {
                                        return Err("unpaired surrogate".into());
                                    }
                                    code = 0x10000 + ((code - 0xD800) << 10) + (low - 0xDC00);
                                }
                                out.push(char::from_u32(code).ok_or("bad \\u escape")?);
                            }
                            _ => return Err(format!("bad escape at byte {}", self.i - 1)),
                        }
                        run = self.i;
                    }
                    Some(c) if *c < 0x20 => return Err("a control character in a string".into()),
                    Some(_) => self.i += 1,
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::net::TcpListener;
    use std::thread;

    use super::*;
    use crate::adapter::GraniteAdapter;

    fn request(prompt: &str, max_tokens: u32) -> PreparedRequest {
        PreparedRequest {
            prompt: prompt.into(),
            max_tokens,
            quant: Quant::Gguf,
        }
    }

    /// A sidecar that answers one connection with `response`, handing back the request
    /// it received.
    fn sidecar(response: Vec<u8>) -> (SocketAddr, thread::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut received = String::new();
            let mut length = 0usize;
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                let lower = line.to_ascii_lowercase();
                if let Some(value) = lower.strip_prefix("content-length:") {
                    length = value.trim().parse().unwrap();
                }
                received.push_str(&line);
                if line == "\r\n" {
                    break;
                }
            }
            let mut body = vec![0u8; length];
            reader.read_exact(&mut body).unwrap();
            stream.write_all(&response).unwrap();
            received + &String::from_utf8(body).unwrap()
        });
        (addr, handle)
    }

    /// A chunked event stream, each event split across two chunks.
    fn chunked_events(events: &[&str]) -> Vec<u8> {
        let mut out = b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n\
                        Transfer-Encoding: chunked\r\n\r\n"
            .to_vec();
        for event in events {
            let data = format!("data: {event}\n\n");
            let (a, b) = data.as_bytes().split_at(data.len() / 2);
            for part in [a, b] {
                out.extend_from_slice(format!("{:x}\r\n", part.len()).as_bytes());
                out.extend_from_slice(part);
                out.extend_from_slice(b"\r\n");
            }
        }
        out.extend_from_slice(b"0\r\n\r\n");
        out
    }

    fn granite(addr: SocketAddr) -> SidecarBackend {
        SidecarBackend::llama_cpp("llama-cpp/granite", addr, "granite", "Granite")
            .unwrap()
            .with_adapter(Box::new(GraniteAdapter))
    }

    #[test]
    fn streams_a_completion_through_the_family_template() {
        let (addr, served) = sidecar(chunked_events(&[
            r#"{"choices":[{"text":"Hello","index":0}]}"#,
            r#"{"choices":[{"text":", world"}]}"#,
            r#"{"choices":[{"text":"<|end_of_text|>"}],"usage":{"prompt_tokens":12,"completion_tokens":3},"timings":{"cache_n":4}}"#,
            "[DONE]",
        ]));
        let out = granite(addr)
            .infer_batch(&[request("Say \"hi\"\nplease", 16)])
            .unwrap();
        assert_eq!(out[0].text, "Hello, world");
        assert_eq!((out[0].prompt_tokens, out[0].completion_tokens), (12, 3));
        assert!(out[0].cache_hit);

        let received = served.join().unwrap();
        assert!(received.starts_with("POST /v1/completions HTTP/1.1\r\n"));
        assert!(received.contains(r#""model":"granite""#));
        assert!(received.contains(r#""max_tokens":16"#));
        assert!(received.contains(r#""stream":true"#));
        assert!(received.contains(r#"<|start_of_role|>user<|end_of_role|>Say \"hi\"\nplease"#));
        assert!(received.contains(r#""stop":["#));
    }

    #[test]
    fn counts_pieces_when_the_engine_reports_no_usage() {
        let body = "data: {\"choices\":[{\"text\":\"one\"}]}\n\n: keep-alive\n\n\
                    data: {\"choices\":[{\"text\":\" two\"}]}\n\ndata: [DONE]\n\n";
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n{body}"
        );
        let (addr, served) = sidecar(response.into_bytes());
        let backend = SidecarBackend::vllm(
            "vllm/mistral",
            addr,
            "mistral",
            vec!["Mistral".into()],
            HwProfile::none(),
            20,
        )
        .unwrap();
        let out = backend.infer_batch(&[request("a b c", 8)]).unwrap();
        assert_eq!(out[0].text, "one two");
        assert_eq!((out[0].prompt_tokens, out[0].completion_tokens), (3, 2));
        assert!(served.join().unwrap().contains(r#""include_usage":true"#));
    }

    #[test]
    fn fails_closed_on_a_refusal_a_malformed_event_or_a_cut_stream() {
        let refusal = "{\"error\":\"Loading model\"}";
        let (addr, _) = sidecar(
            format!(
                "HTTP/1.1 503 Service Unavailable\r\nContent-Length: {}\r\n\r\n{refusal}",
                refusal.len()
            )
            .into_bytes(),
        );
        let e = granite(addr).infer_batch(&[request("x", 4)]).unwrap_err();
        assert!(
            matches!(&e, InferError::Unavailable(m) if m.contains("HTTP 503") && m.contains("Loading model")),
            "{e}"
        );

        let (addr, _) = sidecar(chunked_events(&[r#"{"choices":["#]));
        let e = granite(addr).infer_batch(&[request("x", 4)]).unwrap_err();
        assert!(e.to_string().contains("malformed event"), "{e}");

        let (addr, _) = sidecar(chunked_events(&[r#"{"choices":[{"text":"partial"}]}"#]));
        let e = granite(addr).infer_batch(&[request("x", 4)]).unwrap_err();
        assert!(e.to_string().contains("before [DONE]"), "{e}");

        let (addr, _) = sidecar(chunked_events(&[
            r#"{"error":{"message":"out of memory"}}"#,
        ]));
        let e = granite(addr).infer_batch(&[request("x", 4)]).unwrap_err();
        assert!(e.to_string().contains("out of memory"), "{e}");
    }

    #[test]
    fn refuses_a_non_loopback_sidecar_and_fails_closed_when_unreachable() {
        assert!(
            SidecarBackend::llama_cpp("x", "10.0.0.5:8080".parse().unwrap(), "m", "Granite")
                .is_err()
        );

        let closed = TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap();
        let backend = granite(closed);
        assert!(matches!(
            backend.infer_batch(&[request("x", 4)]),
            Err(InferError::Unavailable(_))
        ));
        assert!(!backend.healthy());
    }

    #[test]
    fn health_is_a_200_from_the_sidecar() {
        let (addr, served) =
            sidecar(b"HTTP/1.1 200 OK\r\nContent-Length: 15\r\n\r\n{\"status\":\"ok\"}".to_vec());
        assert!(granite(addr).healthy());
        assert!(served
            .join()
            .unwrap()
            .starts_with("GET /health HTTP/1.1\r\n"));

        let (addr, _) =
            sidecar(b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\n\r\n".to_vec());
        assert!(!granite(addr).healthy());
    }

    #[test]
    fn json_round_trips_escapes_and_refuses_junk() {
        let v =
            json::parse(r#"{"a":"\u00e9\ud83d\ude00\"x\/","n":[1,2.5,-3e2,true,null]}"#).unwrap();
        assert_eq!(v.get("a").and_then(|a| a.as_str()), Some("é😀\"x/"));
        assert_eq!(
            v.get("n").and_then(|n| n.first()).and_then(|n| n.as_u64()),
            Some(1)
        );
        let text = "tab\tquote\" back\\ nul\u{0}";
        assert_eq!(
            json::parse(&json::string(text)).unwrap().as_str(),
            Some(text)
        );
        for junk in [
            r#"{"a":1,}"#,
            r#"{"a":1} x"#,
            "\"\\ud800\"",
            "[1,",
            "\"a\u{1}\"",
        ] {
            assert!(json::parse(junk).is_err(), "{junk:?}");
        }
    }
}
