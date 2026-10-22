//! Structured logging. A [`LogRecord`] is a level + target + message + ordered
//! key/value fields, optionally carrying the active trace/span ids so logs correlate
//! with traces. Records go to a [`LogSink`]; the shipped sinks write **inside the
//! enclosure** — a buffer (tests) or stdout as JSON lines (captured by the local log
//! collector). There is no network sink. [NFR-PRV-1] [RC-33]

use std::sync::Mutex;

/// Log severity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    /// Fine-grained diagnostic.
    Debug,
    /// Normal operation.
    Info,
    /// Something unexpected but handled.
    Warn,
    /// A failure.
    Error,
}

impl Level {
    /// The lowercase level name.
    pub fn as_str(&self) -> &'static str {
        match self {
            Level::Debug => "debug",
            Level::Info => "info",
            Level::Warn => "warn",
            Level::Error => "error",
        }
    }
}

/// A single structured log record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogRecord {
    /// Severity.
    pub level: Level,
    /// The emitting subsystem, e.g. `http-gateway`.
    pub target: String,
    /// The message.
    pub message: String,
    /// Ordered structured fields.
    pub fields: Vec<(String, String)>,
    /// The active trace id (hex), if any.
    pub trace_id: Option<String>,
    /// The active span id (hex), if any.
    pub span_id: Option<String>,
}

impl LogRecord {
    /// Render as one JSON line (no serde).
    pub fn to_json(&self) -> String {
        let mut o = String::from("{");
        o.push_str(&format!("\"level\":\"{}\"", self.level.as_str()));
        o.push_str(&format!(",\"target\":\"{}\"", esc(&self.target)));
        o.push_str(&format!(",\"message\":\"{}\"", esc(&self.message)));
        if let Some(t) = &self.trace_id {
            o.push_str(&format!(",\"trace_id\":\"{}\"", esc(t)));
        }
        if let Some(s) = &self.span_id {
            o.push_str(&format!(",\"span_id\":\"{}\"", esc(s)));
        }
        for (k, v) in &self.fields {
            o.push_str(&format!(",\"{}\":\"{}\"", esc(k), esc(v)));
        }
        o.push('}');
        o
    }
}

/// A destination for log records — always in-enclosure.
pub trait LogSink: Send + Sync {
    /// Emit one record.
    fn emit(&self, record: &LogRecord);
}

/// A buffer sink (tests / in-memory capture).
#[derive(Default)]
pub struct BufferSink {
    records: Mutex<Vec<LogRecord>>,
}

impl BufferSink {
    /// A new empty buffer.
    pub fn new() -> Self {
        Self::default()
    }
    /// A snapshot of the captured records.
    pub fn records(&self) -> Vec<LogRecord> {
        self.records.lock().expect("log buffer").clone()
    }
    /// The number of records captured.
    pub fn len(&self) -> usize {
        self.records.lock().expect("log buffer").len()
    }
    /// Whether nothing has been logged.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl LogSink for BufferSink {
    fn emit(&self, record: &LogRecord) {
        self.records
            .lock()
            .expect("log buffer")
            .push(record.clone());
    }
}

/// A sink that writes each record as a JSON line to stdout — captured by the local,
/// in-enclosure log collector. No network. [NFR-PRV-1]
pub struct StdoutJsonSink;

impl LogSink for StdoutJsonSink {
    fn emit(&self, record: &LogRecord) {
        write_line(&mut std::io::stdout().lock(), record);
    }
}

/// Write `record` as one JSON line. A line that cannot be written (stdout closed, a
/// full pipe) is dropped: logging must never take the service down.
fn write_line(out: &mut dyn std::io::Write, record: &LogRecord) {
    let mut line = record.to_json();
    line.push('\n');
    let _ = out.write_all(line.as_bytes());
}

/// A logger bound to a target and a sink, optionally stamping a trace/span context.
pub struct Logger<'a> {
    target: String,
    sink: &'a dyn LogSink,
    trace_id: Option<String>,
    span_id: Option<String>,
}

impl<'a> Logger<'a> {
    /// A logger for `target` over `sink`.
    pub fn new(target: &str, sink: &'a dyn LogSink) -> Self {
        Self {
            target: target.to_string(),
            sink,
            trace_id: None,
            span_id: None,
        }
    }

    /// Attach a trace/span context so emitted logs correlate with the trace.
    pub fn with_context(mut self, trace_id: &str, span_id: &str) -> Self {
        self.trace_id = Some(trace_id.to_string());
        self.span_id = Some(span_id.to_string());
        self
    }

    /// Emit a record at `level` with structured `fields`.
    pub fn log(&self, level: Level, message: &str, fields: &[(&str, &str)]) {
        let record = LogRecord {
            level,
            target: self.target.clone(),
            message: message.to_string(),
            fields: fields
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            trace_id: self.trace_id.clone(),
            span_id: self.span_id.clone(),
        };
        self.sink.emit(&record);
    }

    /// Info-level convenience.
    pub fn info(&self, message: &str, fields: &[(&str, &str)]) {
        self.log(Level::Info, message, fields);
    }
    /// Warn-level convenience.
    pub fn warn(&self, message: &str, fields: &[(&str, &str)]) {
        self.log(Level::Warn, message, fields);
    }
    /// Error-level convenience.
    pub fn error(&self, message: &str, fields: &[(&str, &str)]) {
        self.log(Level::Error, message, fields);
    }
}

/// Escape `s` for a JSON string: quotes, backslashes and every control character, so
/// a request path or header cannot break or forge a log line.
fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => o.push_str("\\\\"),
            '"' => o.push_str("\\\""),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"),
            c if (c as u32) < 0x20 => o.push_str(&format!("\\u{:04x}", c as u32)),
            c => o.push(c),
        }
    }
    o
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(message: &str) -> LogRecord {
        LogRecord {
            level: Level::Info,
            target: "t".into(),
            message: message.into(),
            fields: vec![("path".into(), "/a\tb".into())],
            trace_id: None,
            span_id: None,
        }
    }

    #[test]
    fn every_control_character_is_escaped() {
        let json = record("a\"b\\c\nd\re\tf\u{0}g\u{1b}h\u{1f}i\u{7f}").to_json();
        assert!(
            json.contains(r#""message":"a\"b\\c\nd\re\tf\u0000g\u001bh\u001fi"#),
            "{json}"
        );
        assert!(json.contains(r#""path":"/a\tb""#), "{json}");
        assert!(
            !json.chars().any(|c| (c as u32) < 0x20),
            "no raw control character survives: {json:?}"
        );
    }

    struct Broken;
    impl std::io::Write for Broken {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "closed",
            ))
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn a_log_line_that_cannot_be_written_is_dropped_not_a_panic() {
        write_line(&mut Broken, &record("lost"));
        let mut out = Vec::new();
        write_line(&mut out, &record("kept"));
        assert_eq!(
            String::from_utf8(out).unwrap(),
            record("kept").to_json() + "\n"
        );
    }
}
