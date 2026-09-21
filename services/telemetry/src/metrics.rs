//! Metrics: counters, gauges, and histograms rendered in the **Prometheus text
//! exposition format** for a `/metrics` scrape. The registry is the single mutation
//! point (thread-safe), so instrumentation is a one-liner and rendering is
//! deterministic. Nothing is pushed anywhere — a scraper *inside the enclosure*
//! pulls `/metrics`; there is no outbound client. [NFR-PRV-1] [RC-33]

use std::collections::BTreeMap;
use std::sync::Mutex;

/// A label set, kept sorted so a metric series has one canonical key.
type Labels = Vec<(String, String)>;

fn norm(labels: &[(&str, &str)]) -> Labels {
    let mut v: Labels = labels
        .iter()
        .map(|(k, val)| (k.to_string(), val.to_string()))
        .collect();
    v.sort();
    v
}

/// Fixed histogram bucket upper bounds (milliseconds), covering sub-ms to ~30s.
pub const DEFAULT_BUCKETS_MS: &[f64] = &[
    1.0, 5.0, 10.0, 25.0, 50.0, 100.0, 250.0, 500.0, 1000.0, 2500.0, 5000.0, 10000.0, 30000.0,
];

#[derive(Default)]
struct Histogram {
    bounds: Vec<f64>,
    /// `bucket_hits[i]` = count of observations ≤ `bounds[i]` (the `le` cumulative).
    bucket_hits: Vec<u64>,
    sum: f64,
    count: u64,
}

impl Histogram {
    fn new(bounds: &[f64]) -> Self {
        Self {
            bounds: bounds.to_vec(),
            bucket_hits: vec![0; bounds.len()],
            sum: 0.0,
            count: 0,
        }
    }
    fn observe(&mut self, v: f64) {
        self.sum += v;
        self.count += 1;
        for (i, &b) in self.bounds.iter().enumerate() {
            if v <= b {
                self.bucket_hits[i] += 1;
            }
        }
    }
}

enum Series {
    Counter(u64),
    Gauge(f64),
    Hist(Histogram),
}

struct Family {
    kind: &'static str, // "counter" | "gauge" | "histogram"
    help: String,
    series: BTreeMap<Labels, Series>,
}

/// A thread-safe metrics registry.
#[derive(Default)]
pub struct MetricsRegistry {
    families: Mutex<BTreeMap<String, Family>>,
}

impl MetricsRegistry {
    /// A new, empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    fn family<'a>(
        map: &'a mut BTreeMap<String, Family>,
        name: &str,
        kind: &'static str,
    ) -> &'a mut Family {
        map.entry(name.to_string()).or_insert_with(|| Family {
            kind,
            help: format!("{name} {kind}"),
            series: BTreeMap::new(),
        })
    }

    /// Register help text for a metric family (optional).
    pub fn describe(&self, name: &str, help: &str) {
        let mut map = self.families.lock().expect("metrics lock");
        if let Some(f) = map.get_mut(name) {
            f.help = help.to_string();
        }
    }

    /// Increment a counter by 1.
    pub fn inc(&self, name: &str, labels: &[(&str, &str)]) {
        self.inc_by(name, labels, 1);
    }

    /// Increment a counter by `n`.
    pub fn inc_by(&self, name: &str, labels: &[(&str, &str)], n: u64) {
        let mut map = self.families.lock().expect("metrics lock");
        let f = Self::family(&mut map, name, "counter");
        if let Series::Counter(c) = f.series.entry(norm(labels)).or_insert(Series::Counter(0)) {
            *c += n;
        }
    }

    /// Set a gauge to `v`.
    pub fn set_gauge(&self, name: &str, labels: &[(&str, &str)], v: f64) {
        let mut map = self.families.lock().expect("metrics lock");
        let f = Self::family(&mut map, name, "gauge");
        f.series.insert(norm(labels), Series::Gauge(v));
    }

    /// Observe a value into a histogram (default ms buckets).
    pub fn observe(&self, name: &str, labels: &[(&str, &str)], v: f64) {
        self.observe_with(name, labels, v, DEFAULT_BUCKETS_MS);
    }

    /// Observe with explicit bucket bounds.
    pub fn observe_with(&self, name: &str, labels: &[(&str, &str)], v: f64, bounds: &[f64]) {
        let mut map = self.families.lock().expect("metrics lock");
        let f = Self::family(&mut map, name, "histogram");
        if let Series::Hist(h) = f
            .series
            .entry(norm(labels))
            .or_insert_with(|| Series::Hist(Histogram::new(bounds)))
        {
            h.observe(v);
        }
    }

    /// The current counter value (for assertions / SLO reads).
    pub fn counter_value(&self, name: &str, labels: &[(&str, &str)]) -> u64 {
        let map = self.families.lock().expect("metrics lock");
        match map.get(name).and_then(|f| f.series.get(&norm(labels))) {
            Some(Series::Counter(c)) => *c,
            _ => 0,
        }
    }

    /// Render the whole registry in Prometheus text exposition format. [RC-33]
    pub fn render(&self) -> String {
        let map = self.families.lock().expect("metrics lock");
        let mut out = String::new();
        for (name, f) in map.iter() {
            out.push_str(&format!("# HELP {name} {}\n", f.help));
            out.push_str(&format!("# TYPE {name} {}\n", f.kind));
            for (labels, s) in &f.series {
                match s {
                    Series::Counter(c) => {
                        out.push_str(&format!("{name}{} {c}\n", render_labels(labels, &[])));
                    }
                    Series::Gauge(v) => {
                        out.push_str(&format!(
                            "{name}{} {}\n",
                            render_labels(labels, &[]),
                            fmt(*v)
                        ));
                    }
                    Series::Hist(h) => {
                        // `bucket_hits[i]` already counts observations ≤ bounds[i], and
                        // bounds ascend, so it is the Prometheus `le` cumulative directly.
                        for (i, &b) in h.bounds.iter().enumerate() {
                            out.push_str(&format!(
                                "{name}_bucket{} {}\n",
                                render_labels(labels, &[("le", &fmt(b))]),
                                h.bucket_hits[i]
                            ));
                        }
                        out.push_str(&format!(
                            "{name}_bucket{} {}\n",
                            render_labels(labels, &[("le", "+Inf")]),
                            h.count
                        ));
                        out.push_str(&format!(
                            "{name}_sum{} {}\n",
                            render_labels(labels, &[]),
                            fmt(h.sum)
                        ));
                        out.push_str(&format!(
                            "{name}_count{} {}\n",
                            render_labels(labels, &[]),
                            h.count
                        ));
                    }
                }
            }
        }
        out
    }
}

fn render_labels(base: &Labels, extra: &[(&str, &str)]) -> String {
    let mut all: Vec<(String, String)> = base.clone();
    for (k, v) in extra {
        all.push((k.to_string(), v.to_string()));
    }
    if all.is_empty() {
        return String::new();
    }
    let inner: Vec<String> = all
        .iter()
        .map(|(k, v)| format!("{k}=\"{}\"", escape(v)))
        .collect();
    format!("{{{}}}", inner.join(","))
}

fn escape(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
}

fn fmt(v: f64) -> String {
    if v == v.trunc() && v.is_finite() {
        format!("{}", v as i64)
    } else {
        format!("{v}")
    }
}
