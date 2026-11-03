//! Metrics: counters, gauges, and histograms rendered in the **Prometheus text
//! exposition format** for a `/metrics` scrape. The registry is the single mutation
//! point (thread-safe), so instrumentation is a one-liner and rendering is
//! deterministic. Nothing is pushed anywhere — a scraper *inside the enclosure*
//! pulls `/metrics`; there is no outbound client. [NFR-PRV-1] [RC-33]
//!
//! The registry is bounded whatever its callers label with: a family holds at most
//! [`MAX_SERIES_PER_FAMILY`] series, a label set past that is dropped and counted in
//! `metrics_series_dropped_total{family}`, and a rendered label value is cut at
//! [`MAX_LABEL_VALUE_CHARS`] characters. [VLD-ANB-MIDGARD-0003]

use std::collections::BTreeMap;
use std::sync::Mutex;

use crate::escape::label_value;

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

/// The most series one family holds. A new label set past it is not recorded; the
/// drop is counted in [`DROPPED_SERIES`]. [VLD-ANB-MIDGARD-0003]
pub const MAX_SERIES_PER_FAMILY: usize = 1_000;

/// The counter family that counts series refused at the cap, labelled by `family`.
/// It is exempt from the cap: it holds at most one series per family. [VLD-ANB-MIDGARD-0003]
pub const DROPPED_SERIES: &str = "metrics_series_dropped_total";

/// The longest label value rendered; a longer one is cut there and ends `...`.
/// [VLD-ANB-MIDGARD-0003]
pub const MAX_LABEL_VALUE_CHARS: usize = 256;

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

#[derive(Default)]
struct Families {
    by_name: BTreeMap<String, Family>,
    /// Help text described before its family recorded anything.
    help: BTreeMap<String, String>,
}

/// A thread-safe metrics registry.
#[derive(Default)]
pub struct MetricsRegistry {
    families: Mutex<Families>,
}

impl MetricsRegistry {
    /// A new, empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    fn family<'a>(map: &'a mut Families, name: &str, kind: &'static str) -> &'a mut Family {
        let help = &map.help;
        map.by_name
            .entry(name.to_string())
            .or_insert_with(|| Family {
                kind,
                help: help
                    .get(name)
                    .cloned()
                    .unwrap_or_else(|| format!("{name} {kind}")),
                series: BTreeMap::new(),
            })
    }

    /// The series `labels` of family `name`, made by `new` if it is new and the family
    /// has room. A family at [`MAX_SERIES_PER_FAMILY`] takes no new series: the drop
    /// is counted in [`DROPPED_SERIES`] and `None` returned. [VLD-ANB-MIDGARD-0003]
    fn series<'a>(
        map: &'a mut Families,
        name: &str,
        kind: &'static str,
        labels: &[(&str, &str)],
        new: impl FnOnce() -> Series,
    ) -> Option<&'a mut Series> {
        let key = norm(labels);
        let f = Self::family(map, name, kind);
        if name != DROPPED_SERIES
            && f.series.len() >= MAX_SERIES_PER_FAMILY
            && !f.series.contains_key(&key)
        {
            let dropped = Self::family(map, DROPPED_SERIES, "counter")
                .series
                .entry(norm(&[("family", name)]))
                .or_insert(Series::Counter(0));
            if let Series::Counter(c) = dropped {
                *c += 1;
            }
            return None;
        }
        Some(
            Self::family(map, name, kind)
                .series
                .entry(key)
                .or_insert_with(new),
        )
    }

    /// Register help text for a metric family (optional). It may come before the
    /// family records anything.
    pub fn describe(&self, name: &str, help: &str) {
        let mut map = self.families.lock().expect("metrics lock");
        match map.by_name.get_mut(name) {
            Some(f) => f.help = help.to_string(),
            None => {
                map.help.insert(name.to_string(), help.to_string());
            }
        }
    }

    /// How many series family `name` holds (for assertions). [VLD-ANB-MIDGARD-0003]
    pub fn series_count(&self, name: &str) -> usize {
        let map = self.families.lock().expect("metrics lock");
        map.by_name.get(name).map_or(0, |f| f.series.len())
    }

    /// Increment a counter by 1.
    pub fn inc(&self, name: &str, labels: &[(&str, &str)]) {
        self.inc_by(name, labels, 1);
    }

    /// Increment a counter by `n`.
    pub fn inc_by(&self, name: &str, labels: &[(&str, &str)], n: u64) {
        let mut map = self.families.lock().expect("metrics lock");
        if let Some(Series::Counter(c)) =
            Self::series(&mut map, name, "counter", labels, || Series::Counter(0))
        {
            *c += n;
        }
    }

    /// Set a gauge to `v`.
    pub fn set_gauge(&self, name: &str, labels: &[(&str, &str)], v: f64) {
        let mut map = self.families.lock().expect("metrics lock");
        if let Some(s) = Self::series(&mut map, name, "gauge", labels, || Series::Gauge(v)) {
            *s = Series::Gauge(v);
        }
    }

    /// Observe a value into a histogram (default ms buckets).
    pub fn observe(&self, name: &str, labels: &[(&str, &str)], v: f64) {
        self.observe_with(name, labels, v, DEFAULT_BUCKETS_MS);
    }

    /// Observe with explicit bucket bounds.
    pub fn observe_with(&self, name: &str, labels: &[(&str, &str)], v: f64, bounds: &[f64]) {
        let mut map = self.families.lock().expect("metrics lock");
        if let Some(Series::Hist(h)) = Self::series(&mut map, name, "histogram", labels, || {
            Series::Hist(Histogram::new(bounds))
        }) {
            h.observe(v);
        }
    }

    /// The current counter value (for assertions / SLO reads).
    pub fn counter_value(&self, name: &str, labels: &[(&str, &str)]) -> u64 {
        let map = self.families.lock().expect("metrics lock");
        match map
            .by_name
            .get(name)
            .and_then(|f| f.series.get(&norm(labels)))
        {
            Some(Series::Counter(c)) => *c,
            _ => 0,
        }
    }

    /// Render the whole registry in Prometheus text exposition format. [RC-33]
    pub fn render(&self) -> String {
        let map = self.families.lock().expect("metrics lock");
        let mut out = String::new();
        for (name, f) in map.by_name.iter() {
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
        .map(|(k, v)| format!("{k}=\"{}\"", label_value(&truncate(v))))
        .collect();
    format!("{{{}}}", inner.join(","))
}

/// `v`, cut at [`MAX_LABEL_VALUE_CHARS`] characters with `...` appended if it is
/// longer: a second line of defence for a caller that labels with input.
/// [VLD-ANB-MIDGARD-0003]
fn truncate(v: &str) -> std::borrow::Cow<'_, str> {
    match v.char_indices().nth(MAX_LABEL_VALUE_CHARS) {
        Some((cut, _)) => format!("{}...", &v[..cut]).into(),
        None => v.into(),
    }
}

fn fmt(v: f64) -> String {
    if v == v.trunc() && v.is_finite() {
        format!("{}", v as i64)
    } else {
        format!("{v}")
    }
}
