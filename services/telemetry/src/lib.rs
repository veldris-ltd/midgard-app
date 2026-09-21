//! # telemetry — in-enclosure observability
//!
//! A self-contained, **zero-dependency** (pure-std) observability stack for the
//! Midgard services. Because it has no dependencies it has no network client and
//! **cannot egress**, which is exactly the air-gap guarantee NFR-PRV-1 demands: a
//! scraper or collector *inside the enclosure* pulls what it needs; nothing is ever
//! pushed out. [NFR-PRV-1] [RC-33]
//!
//! Four pillars, each a thin seam over an in-enclosure sink:
//! - [`metrics`] — counters/gauges/histograms rendered as Prometheus text (`/metrics`);
//! - [`health`] — liveness (`/health`) + readiness (`/ready`) probes;
//! - [`trace`] — W3C-`traceparent` distributed tracing to a buffered exporter;
//! - [`log`] — structured JSON logging to a buffered / stdout sink;
//! - [`slo`] — availability + latency SLO trackers whose breaches surface as metrics.

pub mod health;
pub mod log;
pub mod metrics;
pub mod slo;
pub mod trace;

pub use health::{CheckResult, HealthRegistry, HealthReport, ReadinessCheck, Toggle};
pub use log::{BufferSink, Level, LogRecord, LogSink, Logger, StdoutJsonSink};
pub use metrics::{MetricsRegistry, DEFAULT_BUCKETS_MS};
pub use slo::{AvailabilitySlo, LatencySlo};
pub use trace::{
    BufferExporter, IdGenerator, SpanContext, SpanExporter, SpanId, SpanRecord, TraceId,
};
