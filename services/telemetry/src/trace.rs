//! Distributed tracing. A [`SpanContext`] (128-bit trace id + 64-bit span id) flows
//! across services as a **W3C `traceparent`** header, so a request's path through the
//! enclosure is one correlated trace. Spans are recorded to a [`SpanExporter`] — the
//! shipped exporter buffers in-process; a local collector reads it. No spans leave
//! the enclosure. Ids are generated from a seeded splitmix64 (no external RNG), so
//! traces are deterministic in tests and dependency-free. [NFR-PRV-1] [RC-33]

use std::sync::Mutex;

/// A 128-bit trace id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TraceId(pub [u8; 16]);

/// A 64-bit span id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpanId(pub [u8; 8]);

impl TraceId {
    /// Lowercase-hex (32 chars).
    pub fn hex(&self) -> String {
        hex(&self.0)
    }
    /// Whether this id is all-zero (invalid).
    pub fn is_valid(&self) -> bool {
        self.0.iter().any(|b| *b != 0)
    }
}
impl SpanId {
    /// Lowercase-hex (16 chars).
    pub fn hex(&self) -> String {
        hex(&self.0)
    }
}

/// The propagated span context.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpanContext {
    /// The trace this span belongs to.
    pub trace_id: TraceId,
    /// This span's id.
    pub span_id: SpanId,
}

impl SpanContext {
    /// Render as a W3C `traceparent` header value (`00-trace-span-01`).
    pub fn traceparent(&self) -> String {
        format!("00-{}-{}-01", self.trace_id.hex(), self.span_id.hex())
    }

    /// Parse a W3C `traceparent` header value. Fails closed on any malformed input.
    pub fn parse_traceparent(s: &str) -> Option<SpanContext> {
        let parts: Vec<&str> = s.trim().split('-').collect();
        if parts.len() != 4 || parts[0] != "00" {
            return None;
        }
        let trace = unhex(parts[1])?;
        let span = unhex(parts[2])?;
        if trace.len() != 16 || span.len() != 8 {
            return None;
        }
        let mut t = [0u8; 16];
        t.copy_from_slice(&trace);
        let mut sp = [0u8; 8];
        sp.copy_from_slice(&span);
        let ctx = SpanContext {
            trace_id: TraceId(t),
            span_id: SpanId(sp),
        };
        if !ctx.trace_id.is_valid() {
            return None;
        }
        Some(ctx)
    }
}

/// A recorded span: its context, parent, name, and start/end (caller-supplied ns —
/// the crate does no clock I/O, matching the deterministic-offline posture).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpanRecord {
    /// The span's context.
    pub context: SpanContext,
    /// The parent span id, if any (root spans have none).
    pub parent: Option<SpanId>,
    /// The operation name.
    pub name: String,
    /// Start time (caller clock, ns).
    pub start: u64,
    /// End time (caller clock, ns).
    pub end: u64,
    /// Ordered attributes.
    pub attributes: Vec<(String, String)>,
}

impl SpanRecord {
    /// The span's duration (ns).
    pub fn duration(&self) -> u64 {
        self.end.saturating_sub(self.start)
    }
}

/// A destination for finished spans — always in-enclosure.
pub trait SpanExporter: Send + Sync {
    /// Export one finished span.
    fn export(&self, span: &SpanRecord);
}

/// A buffering exporter (tests / in-process collector).
#[derive(Default)]
pub struct BufferExporter {
    spans: Mutex<Vec<SpanRecord>>,
}

impl BufferExporter {
    /// A new empty exporter.
    pub fn new() -> Self {
        Self::default()
    }
    /// A snapshot of exported spans.
    pub fn spans(&self) -> Vec<SpanRecord> {
        self.spans.lock().expect("span buffer").clone()
    }
    /// The number of spans exported.
    pub fn len(&self) -> usize {
        self.spans.lock().expect("span buffer").len()
    }
    /// Whether nothing has been exported.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl SpanExporter for BufferExporter {
    fn export(&self, span: &SpanRecord) {
        self.spans.lock().expect("span buffer").push(span.clone());
    }
}

/// Generates trace/span ids deterministically from a seed (splitmix64) — zero
/// external dependencies. Production seeds each tracer from `OsRng` at startup; tests
/// use a fixed seed.
pub struct IdGenerator {
    state: Mutex<u64>,
}

impl IdGenerator {
    /// A generator seeded with `seed`.
    pub fn new(seed: u64) -> Self {
        Self {
            state: Mutex::new(seed),
        }
    }

    fn next_u64(&self) -> u64 {
        let mut s = self.state.lock().expect("id gen");
        *s = s.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = *s;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A fresh 128-bit trace id.
    pub fn trace_id(&self) -> TraceId {
        let mut b = [0u8; 16];
        b[..8].copy_from_slice(&self.next_u64().to_le_bytes());
        b[8..].copy_from_slice(&self.next_u64().to_le_bytes());
        TraceId(b)
    }

    /// A fresh 64-bit span id.
    pub fn span_id(&self) -> SpanId {
        SpanId(self.next_u64().to_le_bytes())
    }

    /// Start a new root span context.
    pub fn new_trace(&self) -> SpanContext {
        SpanContext {
            trace_id: self.trace_id(),
            span_id: self.span_id(),
        }
    }

    /// A child span context under `parent` (same trace, new span id).
    pub fn child(&self, parent: &SpanContext) -> SpanContext {
        SpanContext {
            trace_id: parent.trace_id,
            span_id: self.span_id(),
        }
    }
}

fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push(char::from_digit((b >> 4) as u32, 16).unwrap());
        s.push(char::from_digit((b & 0x0f) as u32, 16).unwrap());
    }
    s
}

fn unhex(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 {
        return None;
    }
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(s.len() / 2);
    let mut i = 0;
    while i < b.len() {
        let hi = (b[i] as char).to_digit(16)?;
        let lo = (b[i + 1] as char).to_digit(16)?;
        out.push(((hi << 4) | lo) as u8);
        i += 2;
    }
    Some(out)
}
