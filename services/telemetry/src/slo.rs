//! Service-level objectives (SLOs) and their breach detection. Two shapes cover the [NFR-PRV-1]
//! platform's stated objectives:
//!
//! - **Availability SLO** — the success ratio of requests against an objective (e.g.
//!   99.9%). A breach means too many requests failed.
//! - **Latency SLO** — the fraction of observations under a latency threshold against
//!   an objective (e.g. 99% of first-token latencies under 500 ms — Mimir's
//!   first-token SLO). A breach means the tail blew the budget.
//!
//! Both expose their live state as metrics so a `/metrics` scrape *observes the
//! breach* — no external APM needed. [RC-33]

use crate::metrics::MetricsRegistry;

/// An availability SLO: successes / total against `objective`.
pub struct AvailabilitySlo {
    name: String,
    objective: f64,
    ok: u64,
    total: u64,
}

impl AvailabilitySlo {
    /// Track availability named `name` against `objective` (e.g. 0.999).
    pub fn new(name: &str, objective: f64) -> Self {
        Self {
            name: name.to_string(),
            objective,
            ok: 0,
            total: 0,
        }
    }

    /// Record a served request outcome.
    pub fn record(&mut self, success: bool) {
        self.total += 1;
        if success {
            self.ok += 1;
        }
    }

    /// The observed availability (1.0 when nothing has been recorded yet).
    pub fn availability(&self) -> f64 {
        if self.total == 0 {
            1.0
        } else {
            self.ok as f64 / self.total as f64
        }
    }

    /// Whether the objective is currently breached.
    pub fn is_breaching(&self) -> bool {
        self.availability() < self.objective
    }

    /// Publish this SLO's live state to the metrics registry (availability, objective,
    /// and a breach gauge that a scrape observes). [RC-33]
    pub fn export(&self, m: &MetricsRegistry) {
        let l = &[("slo", self.name.as_str())][..];
        m.set_gauge("slo_availability_ratio", l, self.availability());
        m.set_gauge("slo_availability_objective", l, self.objective);
        m.set_gauge(
            "slo_breaching",
            l,
            if self.is_breaching() { 1.0 } else { 0.0 },
        );
    }
}

/// A latency SLO: fraction of observations `<= threshold` against `objective`.
pub struct LatencySlo {
    name: String,
    threshold_ms: f64,
    objective: f64,
    under: u64,
    total: u64,
    sum_ms: f64,
    max_ms: f64,
}

impl LatencySlo {
    /// Track latency named `name`: at least `objective` of observations must be
    /// `<= threshold_ms` (e.g. 0.99 under 500 ms for first-token). [RC-33]
    pub fn new(name: &str, threshold_ms: f64, objective: f64) -> Self {
        Self {
            name: name.to_string(),
            threshold_ms,
            objective,
            under: 0,
            total: 0,
            sum_ms: 0.0,
            max_ms: 0.0,
        }
    }

    /// Record one latency observation (ms).
    pub fn observe(&mut self, latency_ms: f64) {
        self.total += 1;
        self.sum_ms += latency_ms;
        if latency_ms > self.max_ms {
            self.max_ms = latency_ms;
        }
        if latency_ms <= self.threshold_ms {
            self.under += 1;
        }
    }

    /// The fraction of observations within the threshold (1.0 when none recorded).
    pub fn attainment(&self) -> f64 {
        if self.total == 0 {
            1.0
        } else {
            self.under as f64 / self.total as f64
        }
    }

    /// The mean observed latency (ms).
    pub fn mean_ms(&self) -> f64 {
        if self.total == 0 {
            0.0
        } else {
            self.sum_ms / self.total as f64
        }
    }

    /// The maximum observed latency (ms).
    pub fn max_ms(&self) -> f64 {
        self.max_ms
    }

    /// Whether the latency objective is currently breached.
    pub fn is_breaching(&self) -> bool {
        self.attainment() < self.objective
    }

    /// Publish this SLO's live state to the metrics registry. [RC-33]
    pub fn export(&self, m: &MetricsRegistry) {
        let l = &[("slo", self.name.as_str())][..];
        m.set_gauge("slo_latency_attainment_ratio", l, self.attainment());
        m.set_gauge("slo_latency_objective", l, self.objective);
        m.set_gauge("slo_latency_threshold_ms", l, self.threshold_ms);
        m.set_gauge("slo_latency_mean_ms", l, self.mean_ms());
        m.set_gauge("slo_latency_max_ms", l, self.max_ms);
        m.set_gauge(
            "slo_breaching",
            l,
            if self.is_breaching() { 1.0 } else { 0.0 },
        );
    }
}
