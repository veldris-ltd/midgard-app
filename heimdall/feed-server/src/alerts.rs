//! Alerts: what the fused picture says an analyst should look at now, configured by
//! `alerts.json` in the state directory and kept in a sealed [`AlertBook`].
//! [FR-H-1] [FR-H-2] [FR-H-4] [RC-13]
//!
//! ```json
//! { "version": 1,
//!   "labels": { "emergency": "critical", "fast-mover": "info" },
//!   "detectors": {
//!     "went_dark": { "kinds": ["sea-vessel"], "min_speed_kt": 2.0, "severity": "warning" },
//!     "source_disagreement": { "max_km": 5.0, "window_s": 60, "severity": "warning" } } }
//! ```
//!
//! - **Labels.** When an entity's classification becomes a listed label an alert is
//!   raised (one open per entity and label); when the entity no longer has that label,
//!   or expires, the alert is cleared.
//! - **`went_dark`.** When an entity of a listed kind expires by time-to-live while its
//!   last `speed` attribute is at least `min_speed_kt`, a `went-dark` alert is raised;
//!   it clears when the entity reappears or is acknowledged.
//! - **`source_disagreement`.** When two different feeds report one entity's position
//!   within `window_s` seconds of each other (observation time) more than `max_km`
//!   apart, a `source-disagreement` alert is raised (one open per entity); it clears
//!   when the feeds have agreed for [`AGREEMENT_CLEARS_SECS`], or when acknowledged
//!   (after which it is not raised again for that entity until its feeds next agree).
//!
//! Both detectors and `labels` are optional; unknown keys, an unknown severity, a
//! label given twice or a label that names a detector refuse the file. Acknowledging
//! an alert records who and when; it clears a detector's alert, but a label alert
//! stays open until the label goes. Cleared alerts are pruned
//! [`CLEARED_RETENTION_SECS`] after clearing. The caller audits every
//! [`AlertChange`] the book reports.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use audit_writer::Sealer;
use heimdall_picture::{Attr, Observation, TrackedEntity};
use service_runtime::json::{parse, Json};

/// The label of a [`WentDark`] alert.
pub const WENT_DARK: &str = "went-dark";
/// The label of a [`Disagreement`] alert.
pub const SOURCE_DISAGREEMENT: &str = "source-disagreement";
/// How long a cleared alert is kept, in seconds (a day).
pub const CLEARED_RETENTION_SECS: u64 = 86_400;
/// How long two feeds must agree for a disagreement alert to clear, in seconds.
pub const AGREEMENT_CLEARS_SECS: u64 = 300;

const VERSION: i64 = 1;
const AAD: &[u8] = b"midgard-heimdall-alerts-v1";

/// How urgent an alert is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    /// Act now.
    Critical,
    /// Look soon.
    Warning,
    /// For awareness.
    Info,
}

impl Severity {
    /// The wire name.
    pub fn as_str(self) -> &'static str {
        match self {
            Severity::Critical => "critical",
            Severity::Warning => "warning",
            Severity::Info => "info",
        }
    }

    fn parse(s: &str) -> Option<Self> {
        match s {
            "critical" => Some(Severity::Critical),
            "warning" => Some(Severity::Warning),
            "info" => Some(Severity::Info),
            _ => None,
        }
    }
}

/// The `went_dark` detector.
#[derive(Debug, Clone, PartialEq)]
pub struct WentDark {
    /// The exact kinds it watches.
    pub kinds: BTreeSet<String>,
    /// The least last speed, in knots, that makes going quiet suspicious.
    pub min_speed_kt: f64,
    /// Only entities that were reported steadily count: at least this many reports
    /// (`min_reports`) at no less than this rate (`min_reports_per_hour`) over their life.
    /// Something that drifts out of patchy coverage has few, sparse reports, so its going
    /// quiet is coverage, not behaviour. Both default to 0 (off).
    pub min_reports: u64,
    pub min_reports_per_hour: f64,
    /// The alert's severity.
    pub severity: Severity,
}

/// The `source_disagreement` detector.
#[derive(Debug, Clone, PartialEq)]
pub struct Disagreement {
    /// The furthest apart two feeds' positions may be, in km.
    pub max_km: f64,
    /// How close in observation time two reports must be to be compared, in seconds.
    pub window_s: u64,
    /// The alert's severity.
    pub severity: Severity,
}

/// What raises alerts.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AlertConfig {
    /// Classification labels that raise an alert, and its severity.
    pub labels: BTreeMap<String, Severity>,
    /// The `went_dark` detector, if configured.
    pub went_dark: Option<WentDark>,
    /// The `source_disagreement` detector, if configured.
    pub source_disagreement: Option<Disagreement>,
}

impl AlertConfig {
    /// One log field: `labels=emergency:critical,...;went_dark;source_disagreement`.
    pub fn describe(&self) -> String {
        let labels: Vec<String> = self
            .labels
            .iter()
            .map(|(l, s)| format!("{l}:{}", s.as_str()))
            .collect();
        let mut parts = vec![format!("labels={}", labels.join(","))];
        if let Some(w) = &self.went_dark {
            let kinds: Vec<&str> = w.kinds.iter().map(String::as_str).collect();
            parts.push(format!(
                "went_dark={}>={}kt",
                kinds.join("|"),
                w.min_speed_kt
            ));
        }
        if let Some(d) = &self.source_disagreement {
            parts.push(format!(
                "source_disagreement>{}km/{}s",
                d.max_km, d.window_s
            ));
        }
        parts.join(";")
    }
}

/// Parse an alerts file.
pub fn parse_alerts(text: &str) -> Result<AlertConfig, String> {
    let doc = parse(text)?;
    only_keys(&doc, "the alerts file", &["version", "labels", "detectors"])?;
    match doc.get("version").and_then(Json::as_i64) {
        Some(VERSION) => {}
        other => {
            return Err(format!(
                "alerts file version must be {VERSION}, not {other:?}"
            ))
        }
    }
    let mut config = AlertConfig::default();
    match doc.get("labels") {
        None => {}
        Some(Json::Obj(pairs)) => {
            for (label, severity) in pairs {
                if label.trim().is_empty() {
                    return Err("labels: a label must be a non-empty string".into());
                }
                if label == WENT_DARK || label == SOURCE_DISAGREEMENT {
                    return Err(format!(
                        "labels: \"{label}\" is a detector's label, not a classification's"
                    ));
                }
                let severity = severity_of(Some(severity), &format!("labels.{label}"))?;
                if config.labels.insert(label.clone(), severity).is_some() {
                    return Err(format!("labels: \"{label}\" is given twice"));
                }
            }
        }
        Some(_) => return Err("\"labels\" must be an object".into()),
    }
    let Some(detectors) = doc.get("detectors") else {
        return Ok(config);
    };
    only_keys(
        detectors,
        "detectors",
        &["went_dark", "source_disagreement"],
    )?;
    if let Some(w) = detectors.get("went_dark") {
        let at = "detectors.went_dark";
        only_keys(
            w,
            at,
            &[
                "kinds",
                "min_speed_kt",
                "min_reports",
                "min_reports_per_hour",
                "severity",
            ],
        )?;
        let Some(Json::Arr(items)) = w.get("kinds") else {
            return Err(format!("{at}: \"kinds\" must be an array of kinds"));
        };
        let mut kinds = BTreeSet::new();
        for item in items {
            match item {
                Json::Str(k) if !k.trim().is_empty() => {
                    kinds.insert(k.clone());
                }
                _ => return Err(format!("{at}: each kind must be a non-empty string")),
            }
        }
        if kinds.is_empty() {
            return Err(format!("{at}: \"kinds\" must not be empty"));
        }
        let min_speed_kt = number(w, at, "min_speed_kt")?;
        if min_speed_kt < 0.0 {
            return Err(format!("{at}: \"min_speed_kt\" must not be negative"));
        }
        let optional = |key: &str| -> Result<f64, String> {
            match w.get(key) {
                None => Ok(0.0),
                Some(_) => {
                    let v = number(w, at, key)?;
                    if v < 0.0 {
                        return Err(format!("{at}: \"{key}\" must not be negative"));
                    }
                    Ok(v)
                }
            }
        };
        let min_reports = optional("min_reports")?;
        if min_reports.fract() != 0.0 {
            return Err(format!("{at}: \"min_reports\" must be a whole number"));
        }
        config.went_dark = Some(WentDark {
            kinds,
            min_speed_kt,
            min_reports: min_reports as u64,
            min_reports_per_hour: optional("min_reports_per_hour")?,
            severity: severity_of(w.get("severity"), at)?,
        });
    }
    if let Some(d) = detectors.get("source_disagreement") {
        let at = "detectors.source_disagreement";
        only_keys(d, at, &["max_km", "window_s", "severity"])?;
        let max_km = number(d, at, "max_km")?;
        if max_km <= 0.0 {
            return Err(format!("{at}: \"max_km\" must be positive"));
        }
        let window_s = match d.get("window_s") {
            Some(Json::Int(n)) if *n > 0 => *n as u64,
            _ => {
                return Err(format!(
                    "{at}: \"window_s\" must be a positive whole number of seconds"
                ))
            }
        };
        config.source_disagreement = Some(Disagreement {
            max_km,
            window_s,
            severity: severity_of(d.get("severity"), at)?,
        });
    }
    Ok(config)
}

fn severity_of(value: Option<&Json>, at: &str) -> Result<Severity, String> {
    value
        .and_then(Json::as_str)
        .and_then(Severity::parse)
        .ok_or_else(|| format!("{at}: severity must be \"critical\", \"warning\" or \"info\""))
}

fn number(item: &Json, at: &str, key: &str) -> Result<f64, String> {
    match item.get(key) {
        Some(Json::Int(i)) => Ok(*i as f64),
        Some(Json::Num(n)) if n.is_finite() => Ok(*n),
        _ => Err(format!("{at}: \"{key}\" must be a number")),
    }
}

fn only_keys(item: &Json, at: &str, allowed: &[&str]) -> Result<(), String> {
    let Json::Obj(pairs) = item else {
        return Err(format!("{at} must be an object"));
    };
    let mut seen = BTreeSet::new();
    for (key, _) in pairs {
        if !allowed.contains(&key.as_str()) {
            return Err(format!("{at}: unknown key \"{key}\""));
        }
        if !seen.insert(key.as_str()) {
            return Err(format!("{at}: \"{key}\" is given twice"));
        }
    }
    Ok(())
}

/// One alert, as [`crate::READ_ALERTS`] serves it.
#[derive(Debug, Clone, PartialEq)]
pub struct Alert {
    /// `A-<n>`, in the order raised.
    pub id: String,
    /// The entity id, as the picture reports it.
    pub entity: String,
    /// The entity's kind.
    pub kind: String,
    /// The classification label, or [`WENT_DARK`] / [`SOURCE_DISAGREEMENT`].
    pub label: String,
    /// How urgent it is.
    pub severity: Severity,
    /// Why it was raised.
    pub rationale: String,
    /// When it was raised (service clock, unix seconds).
    pub raised_at: u64,
    /// When it was cleared, if it has been.
    pub cleared_at: Option<u64>,
    /// Who acknowledged it, if anyone has.
    pub acknowledged_by: Option<String>,
    /// When it was acknowledged.
    pub acknowledged_at: Option<u64>,
    /// The entity's latitude when raised, if known.
    pub lat: Option<f64>,
    /// The entity's longitude when raised, if known.
    pub lon: Option<f64>,
}

impl Alert {
    /// Not yet cleared (acknowledged or not).
    pub fn is_open(&self) -> bool {
        self.cleared_at.is_none()
    }

    fn is_label_alert(&self) -> bool {
        self.label != WENT_DARK && self.label != SOURCE_DISAGREEMENT
    }

    /// The wire form: `{"id","entity","kind","label","severity","rationale","raisedAt",
    /// "clearedAt","acknowledgedBy","acknowledgedAt","lat","lon"}`.
    pub fn to_json(&self) -> Json {
        let secs = |v: Option<u64>| v.map_or(Json::Null, |v| Json::Int(v as i64));
        let num = |v: Option<f64>| v.map_or(Json::Null, Json::Num);
        Json::obj(vec![
            ("id", Json::s(&self.id)),
            ("entity", Json::s(&self.entity)),
            ("kind", Json::s(&self.kind)),
            ("label", Json::s(&self.label)),
            ("severity", Json::s(self.severity.as_str())),
            ("rationale", Json::s(&self.rationale)),
            ("raisedAt", Json::Int(self.raised_at as i64)),
            ("clearedAt", secs(self.cleared_at)),
            (
                "acknowledgedBy",
                self.acknowledged_by.as_deref().map_or(Json::Null, Json::s),
            ),
            ("acknowledgedAt", secs(self.acknowledged_at)),
            ("lat", num(self.lat)),
            ("lon", num(self.lon)),
        ])
    }

    fn from_json(j: &Json) -> Option<Self> {
        let text = |k: &str| j.str_field(k).map(str::to_string);
        let secs = |k: &str| match j.get(k) {
            Some(Json::Int(n)) if *n >= 0 => Some(Some(*n as u64)),
            Some(Json::Null) => Some(None),
            _ => None,
        };
        let num = |k: &str| match j.get(k) {
            Some(Json::Num(n)) => Some(Some(*n)),
            Some(Json::Int(n)) => Some(Some(*n as f64)),
            Some(Json::Null) => Some(None),
            _ => None,
        };
        Some(Self {
            id: text("id")?,
            entity: text("entity")?,
            kind: text("kind")?,
            label: text("label")?,
            severity: Severity::parse(j.str_field("severity")?)?,
            rationale: text("rationale")?,
            raised_at: secs("raisedAt")??,
            cleared_at: secs("clearedAt")?,
            acknowledged_by: match j.get("acknowledgedBy") {
                Some(Json::Str(s)) => Some(s.clone()),
                Some(Json::Null) => None,
                _ => return None,
            },
            acknowledged_at: secs("acknowledgedAt")?,
            lat: num("lat")?,
            lon: num("lon")?,
        })
    }
}

/// One change to the book, for the caller to audit.
#[derive(Debug, Clone, PartialEq)]
pub enum AlertChange {
    /// An alert was raised.
    Raised(Alert),
    /// An alert was cleared.
    Cleared(Alert),
}

impl AlertChange {
    /// The audit action: `RaiseAlert` or `ClearAlert`.
    pub fn action(&self) -> &'static str {
        match self {
            AlertChange::Raised(_) => "RaiseAlert",
            AlertChange::Cleared(_) => "ClearAlert",
        }
    }

    /// The alert changed.
    pub fn alert(&self) -> &Alert {
        match self {
            AlertChange::Raised(a) | AlertChange::Cleared(a) => a,
        }
    }

    /// The audit resource: `alert:<id> entity:<id> label:<label>`.
    pub fn resource(&self) -> String {
        let a = self.alert();
        format!("alert:{} entity:{} label:{}", a.id, a.entity, a.label)
    }
}

/// An acknowledgement named no alert the book holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownAlert(pub String);

/// A feed's latest position report for an entity.
struct Report {
    t: u64,
    lat: f64,
    lon: f64,
    pos_source: Option<String>,
}

/// The alerts, newest last, sealed at rest when opened over a file.
pub struct AlertBook {
    config: AlertConfig,
    alerts: Vec<Alert>,
    next_id: u64,
    store: Option<(PathBuf, Box<dyn Sealer>)>,
    dirty: bool,
    changes: Vec<AlertChange>,
    reports: BTreeMap<String, BTreeMap<String, Report>>,
    agreeing_since: BTreeMap<String, u64>,
    suppressed: BTreeSet<String>,
}

impl AlertBook {
    /// A book kept only in memory.
    pub fn in_memory(config: AlertConfig) -> Self {
        Self {
            config,
            alerts: Vec::new(),
            next_id: 1,
            store: None,
            dirty: false,
            changes: Vec::new(),
            reports: BTreeMap::new(),
            agreeing_since: BTreeMap::new(),
            suppressed: BTreeSet::new(),
        }
    }

    /// Open (creating when absent) the sealed book at `path`. Fails closed on at-rest
    /// tamper or corruption.
    pub fn open(
        path: impl AsRef<Path>,
        sealer: Box<dyn Sealer>,
        config: AlertConfig,
    ) -> Result<Self, String> {
        let path = path.as_ref().to_path_buf();
        let mut book = Self::in_memory(config);
        if path.exists() {
            let sealed =
                std::fs::read(&path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
            let plain = sealer
                .unseal(AAD, &sealed)
                .map_err(|e| format!("{} does not unseal: {e}", path.display()))?;
            let text =
                String::from_utf8(plain).map_err(|_| format!("{} is not UTF-8", path.display()))?;
            let doc = parse(&text).map_err(|e| format!("{}: {e}", path.display()))?;
            let bad = || format!("{} is malformed", path.display());
            book.next_id = doc
                .get("next")
                .and_then(Json::as_i64)
                .and_then(|n| u64::try_from(n).ok())
                .ok_or_else(bad)?;
            let Some(Json::Arr(items)) = doc.get("alerts") else {
                return Err(bad());
            };
            for item in items {
                book.alerts.push(Alert::from_json(item).ok_or_else(bad)?);
            }
        }
        book.store = Some((path, sealer));
        Ok(book)
    }

    /// What raises alerts.
    pub fn config(&self) -> &AlertConfig {
        &self.config
    }

    /// Every alert held, oldest first.
    pub fn alerts(&self) -> &[Alert] {
        &self.alerts
    }

    /// The alerts newest first: only open ones, or all.
    pub fn newest_first(&self, open_only: bool) -> impl Iterator<Item = &Alert> {
        self.alerts
            .iter()
            .rev()
            .filter(move |a| !open_only || a.is_open())
    }

    /// The changes since last taken, for the caller to audit.
    pub fn take_changes(&mut self) -> Vec<AlertChange> {
        std::mem::take(&mut self.changes)
    }

    /// Seal the book to its file if anything changed; a failure leaves it to be
    /// written next time.
    pub fn flush(&mut self) -> Result<(), String> {
        if !self.dirty {
            return Ok(());
        }
        if let Some((path, sealer)) = &self.store {
            let doc = Json::obj(vec![
                ("next", Json::Int(self.next_id as i64)),
                (
                    "alerts",
                    Json::Arr(self.alerts.iter().map(Alert::to_json).collect()),
                ),
            ]);
            let sealed = sealer
                .seal(AAD, doc.encode().as_bytes())
                .map_err(|e| format!("the alerts do not seal: {e}"))?;
            atomic_write(path, &sealed)?;
        }
        self.dirty = false;
        Ok(())
    }

    fn has_open(&self, entity: &str, label: &str) -> bool {
        self.alerts
            .iter()
            .any(|a| a.is_open() && a.entity == entity && a.label == label)
    }

    fn raise(
        &mut self,
        entity: &TrackedEntity,
        label: &str,
        severity: Severity,
        rationale: String,
        now: u64,
    ) {
        let (lat, lon) = match position(&entity.latest_attrs) {
            Some((lat, lon)) => (Some(lat), Some(lon)),
            None => (None, None),
        };
        let alert = Alert {
            id: format!("A-{}", self.next_id),
            entity: entity.id.0.clone(),
            kind: entity.kind.clone(),
            label: label.to_string(),
            severity,
            rationale,
            raised_at: now,
            cleared_at: None,
            acknowledged_by: None,
            acknowledged_at: None,
            lat,
            lon,
        };
        self.next_id += 1;
        self.alerts.push(alert.clone());
        self.changes.push(AlertChange::Raised(alert));
        self.dirty = true;
    }

    fn clear_where(&mut self, now: u64, pred: impl Fn(&Alert) -> bool) {
        for a in self.alerts.iter_mut().filter(|a| a.is_open() && pred(a)) {
            a.cleared_at = Some(now);
            self.changes.push(AlertChange::Cleared(a.clone()));
            self.dirty = true;
        }
    }

    /// An entity was fused (`entity` as it now is): its reappearance clears a
    /// `went-dark` alert, and its label alerts follow its classification.
    pub fn entity_updated(&mut self, entity: &TrackedEntity, now: u64) {
        let id = entity.id.0.as_str();
        let current = entity.classification.as_ref().map(|c| c.label.as_str());
        self.clear_where(now, |a| {
            a.entity == id
                && (a.label == WENT_DARK
                    || (a.is_label_alert() && Some(a.label.as_str()) != current))
        });
        if let Some(c) = &entity.classification {
            if let Some(severity) = self.config.labels.get(&c.label).copied() {
                if !self.has_open(id, &c.label) {
                    let rationale = format!("classified {}: {}", c.label, c.rationale);
                    self.raise(entity, &c.label, severity, rationale, now);
                }
            }
        }
    }

    /// An observation of `entity` was fused: compare its position with other feeds'.
    pub fn observed(&mut self, entity: &TrackedEntity, obs: &Observation, now: u64) {
        let Some(cfg) = &self.config.source_disagreement else {
            return;
        };
        let (max_km, window, severity) = (cfg.max_km, cfg.window_s, cfg.severity);
        let Some((lat, lon)) = position(&obs.attrs) else {
            return;
        };
        let id = entity.id.0.clone();
        let feed = obs.feed_id.0.clone();
        let t = obs.observed_at;
        let pos_source = match obs.attrs.get(heimdall_fusion::POS_SOURCE) {
            Some(Attr::Text(s)) => Some(s.clone()),
            _ => None,
        };
        let reports = self.reports.entry(id.clone()).or_default();
        let latest = reports.values().map(|r| r.t).max().unwrap_or(t).max(t);
        reports.retain(|_, r| r.t + window >= latest);
        let mut compared = false;
        let mut worst: Option<(String, f64, Option<String>)> = None;
        for (other, r) in reports.iter() {
            if *other == feed || t.abs_diff(r.t) > window {
                continue;
            }
            compared = true;
            let km = distance_km(lat, lon, r.lat, r.lon);
            if km > max_km && worst.as_ref().is_none_or(|w| km > w.1) {
                worst = Some((other.clone(), km, r.pos_source.clone()));
            }
        }
        reports.insert(
            feed.clone(),
            Report {
                t,
                lat,
                lon,
                pos_source: pos_source.clone(),
            },
        );
        match worst {
            Some((other, km, other_source)) => {
                self.agreeing_since.remove(&id);
                if !self.suppressed.contains(&id) && !self.has_open(&id, SOURCE_DISAGREEMENT) {
                    let src = |s: &Option<String>| s.clone().unwrap_or_else(|| "none".into());
                    let rationale = format!(
                        "feeds {feed} (pos_source {}) and {other} (pos_source {}) place it \
                         {km:.1} km apart",
                        src(&pos_source),
                        src(&other_source)
                    );
                    self.raise(entity, SOURCE_DISAGREEMENT, severity, rationale, now);
                }
            }
            None if compared => {
                self.suppressed.remove(&id);
                let since = *self.agreeing_since.entry(id.clone()).or_insert(t);
                if t.saturating_sub(since) >= AGREEMENT_CLEARS_SECS {
                    self.clear_where(now, |a| a.entity == id && a.label == SOURCE_DISAGREEMENT);
                }
            }
            None => {}
        }
    }

    /// Entities left the picture by time-to-live: their label alerts clear, and one of a
    /// watched kind still under way raises `went-dark`.
    pub fn expired(&mut self, entities: &[TrackedEntity], now: u64) {
        for entity in entities {
            let id = entity.id.0.as_str();
            self.clear_where(now, |a| a.entity == id && a.is_label_alert());
            self.reports.remove(id);
            self.agreeing_since.remove(id);
            self.suppressed.remove(id);
            let Some(w) = &self.config.went_dark else {
                continue;
            };
            let speed = match entity.latest_attrs.get("speed") {
                Some(Attr::Float(f)) if f.is_finite() => *f,
                Some(Attr::Int(i)) => *i as f64,
                _ => continue,
            };
            let reports = entity.track.observation_count;
            let hours = entity
                .track
                .last_seen
                .saturating_sub(entity.track.first_seen) as f64
                / 3600.0;
            let steady = reports >= w.min_reports
                && (w.min_reports_per_hour <= 0.0
                    || (hours > 0.0 && reports as f64 / hours >= w.min_reports_per_hour));
            if w.kinds.contains(&entity.kind)
                && speed >= w.min_speed_kt
                && steady
                && !self.has_open(id, WENT_DARK)
            {
                let severity = w.severity;
                let rationale = format!("stopped reporting while under way at {speed:.1} kt");
                self.raise(entity, WENT_DARK, severity, rationale, now);
            }
        }
    }

    /// Acknowledge alert `id` as `by`: who and when are recorded (the first
    /// acknowledgement stands), and a detector's alert is cleared. Returns the alert as
    /// it now is.
    pub fn acknowledge(&mut self, id: &str, by: &str, now: u64) -> Result<Alert, UnknownAlert> {
        let alert = self
            .alerts
            .iter_mut()
            .find(|a| a.id == id)
            .ok_or_else(|| UnknownAlert(id.to_string()))?;
        if alert.acknowledged_by.is_none() {
            alert.acknowledged_by = Some(by.to_string());
            alert.acknowledged_at = Some(now);
            self.dirty = true;
        }
        if alert.is_open() && !alert.is_label_alert() {
            alert.cleared_at = Some(now);
            self.changes.push(AlertChange::Cleared(alert.clone()));
            self.dirty = true;
            if alert.label == SOURCE_DISAGREEMENT {
                self.suppressed.insert(alert.entity.clone());
                self.agreeing_since.remove(&alert.entity);
            }
        }
        Ok(alert.clone())
    }

    /// Drop alerts cleared [`CLEARED_RETENTION_SECS`] or more before `now`; returns how
    /// many went.
    pub fn prune(&mut self, now: u64) -> usize {
        let before = self.alerts.len();
        self.alerts.retain(|a| {
            a.cleared_at
                .is_none_or(|c| now.saturating_sub(c) < CLEARED_RETENTION_SECS)
        });
        let pruned = before - self.alerts.len();
        if pruned > 0 {
            self.dirty = true;
        }
        pruned
    }
}

/// A numeric `lat` and `lon` in range.
fn position(attrs: &BTreeMap<String, Attr>) -> Option<(f64, f64)> {
    let num = |k: &str| match attrs.get(k) {
        Some(Attr::Float(f)) if f.is_finite() => Some(*f),
        Some(Attr::Int(i)) => Some(*i as f64),
        _ => None,
    };
    let lat = num("lat").filter(|v| (-90.0..=90.0).contains(v))?;
    let lon = num("lon").filter(|v| (-180.0..=180.0).contains(v))?;
    Some((lat, lon))
}

/// Great-circle distance, in km.
pub fn distance_km(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    let (p1, p2) = (lat1.to_radians(), lat2.to_radians());
    let dp = (lat2 - lat1).to_radians();
    let dl = (lon2 - lon1).to_radians();
    let h = (dp / 2.0).sin().powi(2) + p1.cos() * p2.cos() * (dl / 2.0).sin().powi(2);
    2.0 * 6371.0088 * h.sqrt().min(1.0).asin()
}

/// Write `bytes` to `path` atomically: sibling temp file, `fsync`, then rename.
fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    use std::io::Write;
    let tmp = path.with_extension("tmp");
    let fail = |e: std::io::Error| format!("cannot write {}: {e}", path.display());
    {
        let mut opts = std::fs::OpenOptions::new();
        opts.create(true).write(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let mut f = opts.open(&tmp).map_err(fail)?;
        f.write_all(bytes).map_err(fail)?;
        f.sync_all().map_err(fail)?;
    }
    std::fs::rename(&tmp, path).map_err(fail)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use audit_writer::AuditError;
    use heimdall_picture::{
        Classification, ClassificationSource, EntityId, FeedId, LayerId, Track,
    };

    use super::*;

    const CONFIG: &str = r#"{ "version": 1,
        "labels": { "emergency": "critical", "fast-mover": "info" },
        "detectors": {
          "went_dark": { "kinds": ["sea-vessel"], "min_speed_kt": 2.0, "severity": "warning" },
          "source_disagreement": { "max_km": 5.0, "window_s": 60, "severity": "warning" } } }"#;

    fn book() -> AlertBook {
        AlertBook::in_memory(parse_alerts(CONFIG).unwrap())
    }

    fn entity(id: &str, kind: &str, label: Option<&str>, attrs: &[(&str, Attr)]) -> TrackedEntity {
        TrackedEntity {
            id: EntityId(id.into()),
            entity_key: id.into(),
            kind: kind.into(),
            layer: LayerId("air".into()),
            sources: BTreeSet::new(),
            track: Track::opened_at(1),
            classification: label.map(|l| Classification {
                label: l.into(),
                source: ClassificationSource::Rules,
                rationale: "squawk 7700".into(),
            }),
            latest_attrs: attrs
                .iter()
                .map(|(k, v)| (k.to_string(), v.clone()))
                .collect(),
        }
    }

    fn report(feed: &str, t: u64, lat: f64, lon: f64, src: &str) -> Observation {
        Observation::new(
            format!("{feed}-{t}"),
            FeedId(feed.into()),
            "air-track",
            "AC",
            t,
        )
        .with_attr("lat", Attr::Float(lat))
        .with_attr("lon", Attr::Float(lon))
        .with_attr("pos_source", Attr::Text(src.into()))
    }

    fn actions(changes: &[AlertChange]) -> Vec<(&'static str, String)> {
        changes
            .iter()
            .map(|c| (c.action(), c.alert().label.clone()))
            .collect()
    }

    #[test]
    fn the_alerts_file_parses() {
        let c = parse_alerts(CONFIG).unwrap();
        assert_eq!(c.labels.get("emergency"), Some(&Severity::Critical));
        assert_eq!(c.went_dark.as_ref().unwrap().min_speed_kt, 2.0);
        assert_eq!(c.source_disagreement.as_ref().unwrap().window_s, 60);
        let none = parse_alerts(r#"{"version": 1}"#).unwrap();
        assert_eq!(none, AlertConfig::default());
        assert!(parse_alerts(r#"{"version": 1, "detectors": {}}"#).is_ok());
        assert!(
            c.describe().contains("emergency:critical"),
            "{}",
            c.describe()
        );
    }

    #[test]
    fn a_malformed_alerts_file_is_refused() {
        for (bad, why) in [
            (r#"{"version": 2}"#, "version"),
            (r#"{"version": 1, "extra": 1}"#, "unknown key"),
            (r#"{"version": 1, "labels": []}"#, "object"),
            (r#"{"version": 1, "labels": {"x": "urgent"}}"#, "severity"),
            (
                r#"{"version": 1, "labels": {"x": "info", "x": "info"}}"#,
                "twice",
            ),
            (
                r#"{"version": 1, "labels": {"went-dark": "info"}}"#,
                "detector",
            ),
            (r#"{"version": 1, "labels": {"": "info"}}"#, "non-empty"),
            (
                r#"{"version": 1, "detectors": {"dark": {}}}"#,
                "unknown key",
            ),
            (
                r#"{"version": 1, "detectors": {"went_dark": {"kinds": [], "min_speed_kt": 1, "severity": "info"}}}"#,
                "empty",
            ),
            (
                r#"{"version": 1, "detectors": {"went_dark": {"kinds": ["s"], "severity": "info"}}}"#,
                "min_speed_kt",
            ),
            (
                r#"{"version": 1, "detectors": {"went_dark": {"kinds": ["s"], "min_speed_kt": -1, "severity": "info"}}}"#,
                "negative",
            ),
            (
                r#"{"version": 1, "detectors": {"went_dark": {"kinds": ["s"], "min_speed_kt": 1, "severity": "info", "x": 1}}}"#,
                "unknown key",
            ),
            (
                r#"{"version": 1, "detectors": {"source_disagreement": {"max_km": 0, "window_s": 60, "severity": "info"}}}"#,
                "positive",
            ),
            (
                r#"{"version": 1, "detectors": {"source_disagreement": {"max_km": 5, "window_s": 1.5, "severity": "info"}}}"#,
                "window_s",
            ),
            (
                r#"{"version": 1, "detectors": {"source_disagreement": {"max_km": 5, "window_s": 60}}}"#,
                "severity",
            ),
        ] {
            let err = parse_alerts(bad)
                .err()
                .unwrap_or_else(|| panic!("accepted: {bad}"));
            assert!(err.contains(why), "{why}: {err}");
        }
    }

    #[test]
    fn a_label_raises_once_and_clears_when_it_goes() {
        let mut b = book();
        let squawking = entity(
            "AC",
            "air-track",
            Some("emergency"),
            &[("lat", Attr::Float(51.5))],
        );
        b.entity_updated(&squawking, 10);
        let raised = b.take_changes();
        assert_eq!(actions(&raised), [("RaiseAlert", "emergency".to_string())]);
        let a = raised[0].alert();
        assert_eq!(
            (a.id.as_str(), a.severity, a.raised_at),
            ("A-1", Severity::Critical, 10)
        );
        assert_eq!(a.rationale, "classified emergency: squawk 7700");
        assert_eq!((a.lat, a.lon), (None, None), "no lon, so no position");

        b.entity_updated(&squawking, 11);
        assert!(b.take_changes().is_empty(), "no duplicate while open");

        // An unlisted label raises nothing; the old one clears.
        b.entity_updated(&entity("AC", "air-track", Some("surface"), &[]), 12);
        assert_eq!(
            actions(&b.take_changes()),
            [("ClearAlert", "emergency".to_string())]
        );
        assert_eq!(b.alerts()[0].cleared_at, Some(12));

        // Back to emergency: a new alert, since the old one is cleared.
        b.entity_updated(&squawking, 13);
        let again = b.take_changes();
        assert_eq!(again[0].alert().id, "A-2");
        assert_eq!(b.newest_first(true).count(), 1);
        assert_eq!(
            b.newest_first(false).next().unwrap().id,
            "A-2",
            "newest first"
        );

        // Expiry clears it.
        b.expired(&[squawking], 20);
        assert_eq!(
            actions(&b.take_changes()),
            [("ClearAlert", "emergency".to_string())]
        );
        assert_eq!(b.newest_first(true).count(), 0);
    }

    #[test]
    fn a_vessel_under_way_that_expires_went_dark_until_it_reappears_or_is_acknowledged() {
        let mut b = book();
        let under_way = |speed: Attr| {
            entity(
                "V1",
                "sea-vessel",
                None,
                &[
                    ("speed", speed),
                    ("lat", Attr::Float(50.0)),
                    ("lon", Attr::Float(-4.0)),
                ],
            )
        };
        b.expired(
            &[
                under_way(Attr::Float(12.34)),
                entity("V2", "sea-vessel", None, &[("speed", Attr::Float(0.5))]),
                entity("A1", "air-track", None, &[("speed", Attr::Int(400))]),
                entity("V3", "sea-vessel", None, &[]),
            ],
            100,
        );
        let changes = b.take_changes();
        assert_eq!(actions(&changes), [("RaiseAlert", WENT_DARK.to_string())]);
        let a = changes[0].alert();
        assert_eq!(a.entity, "V1");
        assert_eq!(a.rationale, "stopped reporting while under way at 12.3 kt");
        assert_eq!((a.lat, a.lon), (Some(50.0), Some(-4.0)));
        assert_eq!(a.severity, Severity::Warning);

        // Not raised twice; cleared when the vessel reappears.
        b.expired(&[under_way(Attr::Int(10))], 101);
        assert!(b.take_changes().is_empty());
        b.entity_updated(&under_way(Attr::Int(10)), 150);
        assert_eq!(
            actions(&b.take_changes()),
            [("ClearAlert", WENT_DARK.to_string())]
        );

        // Again, then acknowledged: acknowledgement clears a detector's alert.
        b.expired(&[under_way(Attr::Int(10))], 200);
        let id = b.take_changes()[0].alert().id.clone();
        let acked = b.acknowledge(&id, "alice", 210).unwrap();
        assert_eq!(acked.acknowledged_by.as_deref(), Some("alice"));
        assert_eq!(
            (acked.acknowledged_at, acked.cleared_at),
            (Some(210), Some(210))
        );
        assert_eq!(
            actions(&b.take_changes()),
            [("ClearAlert", WENT_DARK.to_string())]
        );
        // A second acknowledgement changes nothing.
        let again = b.acknowledge(&id, "bob", 220).unwrap();
        assert_eq!(again.acknowledged_by.as_deref(), Some("alice"));
        assert!(b.take_changes().is_empty());
        assert_eq!(
            b.acknowledge("A-99", "alice", 1),
            Err(UnknownAlert("A-99".into()))
        );
    }

    #[test]
    fn an_acknowledged_label_alert_stays_open_until_the_label_goes() {
        let mut b = book();
        let e = entity("AC", "air-track", Some("emergency"), &[]);
        b.entity_updated(&e, 1);
        b.take_changes();
        let acked = b.acknowledge("A-1", "alice", 2).unwrap();
        assert!(acked.is_open());
        assert!(b.take_changes().is_empty());
        assert_eq!(b.newest_first(true).count(), 1, "open, acknowledged or not");
    }

    #[test]
    fn feeds_that_disagree_raise_and_clear_after_agreeing_for_five_minutes() {
        let mut b = book();
        let ac = entity("AC", "air-track", None, &[]);
        b.observed(&ac, &report("adsb", 100, 51.0, -1.0, "adsb_icao"), 100);
        // 30 s later another feed puts it ~11 km away.
        b.observed(&ac, &report("mlat", 130, 51.1, -1.0, "mlat"), 130);
        let changes = b.take_changes();
        assert_eq!(
            actions(&changes),
            [("RaiseAlert", SOURCE_DISAGREEMENT.to_string())]
        );
        let rationale = &changes[0].alert().rationale;
        assert!(rationale.contains("mlat (pos_source mlat)"), "{rationale}");
        assert!(
            rationale.contains("adsb (pos_source adsb_icao)"),
            "{rationale}"
        );
        assert!(rationale.contains("11.1 km apart"), "{rationale}");
        // Still disagreeing: no second alert.
        b.observed(&ac, &report("adsb", 140, 51.0, -1.0, "adsb_icao"), 140);
        assert!(b.take_changes().is_empty());

        // A report outside the window is not compared.
        b.observed(&ac, &report("radar", 300, 52.0, -1.0, "radar"), 300);
        assert!(b.take_changes().is_empty());

        // Agreement (within 5 km) for under 5 minutes leaves it open; for 5, clears it.
        // (The radar report has left the window by then.)
        for t in (400..=700).step_by(10) {
            let feed = if t % 20 == 0 { "adsb" } else { "mlat" };
            b.observed(
                &ac,
                &report(feed, t, 51.0, -1.0 + (t % 20) as f64 * 0.001, "x"),
                t,
            );
            assert!(b.take_changes().is_empty(), "{t}");
            assert!(b.has_open("AC", SOURCE_DISAGREEMENT), "{t}");
        }
        b.observed(&ac, &report("mlat", 710, 51.0, -1.0, "mlat"), 710);
        assert_eq!(
            actions(&b.take_changes()),
            [("ClearAlert", SOURCE_DISAGREEMENT.to_string())]
        );
    }

    #[test]
    fn an_acknowledged_disagreement_clears_and_waits_for_agreement() {
        let mut b = book();
        let ac = entity("AC", "air-track", None, &[]);
        b.observed(&ac, &report("adsb", 100, 51.0, -1.0, "adsb_icao"), 100);
        b.observed(&ac, &report("mlat", 101, 52.0, -1.0, "mlat"), 101);
        let id = b.take_changes()[0].alert().id.clone();
        b.acknowledge(&id, "alice", 102).unwrap();
        assert_eq!(
            actions(&b.take_changes()),
            [("ClearAlert", SOURCE_DISAGREEMENT.to_string())]
        );
        b.observed(&ac, &report("mlat", 103, 52.0, -1.0, "mlat"), 103);
        assert!(b.take_changes().is_empty(), "not re-raised at once");
        b.observed(&ac, &report("adsb", 110, 52.0, -1.0, "adsb_icao"), 110);
        b.observed(&ac, &report("mlat", 111, 53.0, -1.0, "mlat"), 111);
        assert_eq!(b.take_changes().len(), 1, "raised again after they agreed");
    }

    /// A stand-in sealer binding the AAD and a checksum.
    struct TestSealer;
    fn sum(aad: &[u8], p: &[u8]) -> u64 {
        aad.iter().chain(p).fold(0xcbf2_9ce4_8422_2325u64, |h, b| {
            (h ^ u64::from(*b)).wrapping_mul(0x100_0000_01b3)
        })
    }
    impl Sealer for TestSealer {
        fn seal(&self, aad: &[u8], plaintext: &[u8]) -> Result<Vec<u8>, AuditError> {
            let mut out = sum(aad, plaintext).to_le_bytes().to_vec();
            out.extend(plaintext.iter().map(|b| b ^ 0x5a));
            Ok(out)
        }
        fn unseal(&self, aad: &[u8], sealed: &[u8]) -> Result<Vec<u8>, AuditError> {
            if sealed.len() < 8 {
                return Err(AuditError::Store("short".into()));
            }
            let plain: Vec<u8> = sealed[8..].iter().map(|b| b ^ 0x5a).collect();
            if sealed[..8] != sum(aad, &plain).to_le_bytes() {
                return Err(AuditError::Store("tampered".into()));
            }
            Ok(plain)
        }
    }

    #[test]
    fn the_book_survives_a_reopen_and_prunes_a_day_after_clearing() {
        let uniq = Box::into_raw(Box::new(0u8)) as usize;
        let dir =
            std::env::temp_dir().join(format!("heimdall-alerts-{}-{uniq}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("alerts.sealed");
        let config = || parse_alerts(CONFIG).unwrap();

        let mut b = AlertBook::open(&path, Box::new(TestSealer), config()).unwrap();
        b.flush().unwrap();
        assert!(!path.exists(), "nothing is written until something changes");
        b.entity_updated(
            &entity(
                "AC",
                "air-track",
                Some("emergency"),
                &[("lat", Attr::Float(1.5)), ("lon", Attr::Int(2))],
            ),
            1_000,
        );
        b.entity_updated(&entity("JET", "air-track", Some("fast-mover"), &[]), 1_001);
        b.acknowledge("A-2", "alice", 1_002).unwrap();
        b.entity_updated(&entity("JET", "air-track", None, &[]), 1_003);
        b.flush().unwrap();
        let before = b.alerts().to_vec();
        drop(b);

        let mut b = AlertBook::open(&path, Box::new(TestSealer), config()).unwrap();
        assert_eq!(b.alerts(), &before[..], "the same alerts after a reopen");
        assert_eq!(b.alerts()[0].lat, Some(1.5));
        assert_eq!(b.alerts()[0].lon, Some(2.0));
        b.entity_updated(&entity("NEW", "air-track", Some("emergency"), &[]), 1_004);
        assert_eq!(b.alerts().last().unwrap().id, "A-3", "ids keep counting");

        assert_eq!(b.prune(1_003 + CLEARED_RETENTION_SECS - 1), 0);
        assert_eq!(
            b.prune(1_003 + CLEARED_RETENTION_SECS),
            1,
            "JET's cleared alert"
        );
        b.flush().unwrap();
        drop(b);
        let b = AlertBook::open(&path, Box::new(TestSealer), config()).unwrap();
        let ids: Vec<&str> = b.alerts().iter().map(|a| a.id.as_str()).collect();
        assert_eq!(ids, ["A-1", "A-3"]);

        // Tampered at rest: refused.
        let mut bytes = std::fs::read(&path).unwrap();
        let last = bytes.len() - 1;
        bytes[last] ^= 1;
        std::fs::write(&path, bytes).unwrap();
        assert!(AlertBook::open(&path, Box::new(TestSealer), config()).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn distance_is_great_circle() {
        let d = distance_km(51.0, -1.0, 51.1, -1.0);
        assert!((d - 11.12).abs() < 0.01, "{d}");
        assert_eq!(distance_km(10.0, 20.0, 10.0, 20.0), 0.0);
    }

    #[test]
    fn went_dark_can_require_steady_reporting() {
        let c = parse_alerts(r#"{"version": 1, "detectors": {"went_dark": {"kinds": ["sea-vessel"], "min_speed_kt": 5, "min_reports": 10, "min_reports_per_hour": 30, "severity": "warning"}}}"#).unwrap();
        let w = c.went_dark.as_ref().unwrap();
        assert_eq!((w.min_reports, w.min_reports_per_hour), (10, 30.0));
        assert!(parse_alerts(r#"{"version": 1, "detectors": {"went_dark": {"kinds": ["s"], "min_speed_kt": 1, "min_reports": 2.5, "severity": "info"}}}"#).is_err());
        let defaults = parse_alerts(r#"{"version": 1, "detectors": {"went_dark": {"kinds": ["s"], "min_speed_kt": 1, "severity": "info"}}}"#).unwrap();
        assert_eq!(defaults.went_dark.unwrap().min_reports, 0);
    }

    #[test]
    fn a_sparsely_reported_vessel_going_quiet_is_not_went_dark() {
        let mut b = AlertBook::in_memory(parse_alerts(r#"{"version": 1, "detectors": {"went_dark": {"kinds": ["sea-vessel"], "min_speed_kt": 2, "min_reports": 10, "min_reports_per_hour": 30, "severity": "warning"}}}"#).unwrap());
        let vessel = |id: &str, reports: u64, minutes: u64| {
            let mut e = entity(id, "sea-vessel", None, &[("speed", Attr::Float(12.0))]);
            e.track = Track {
                first_seen: 0,
                last_seen: minutes * 60,
                observation_count: reports,
            };
            e
        };
        // Steady: 40 reports over 40 minutes (60 an hour). Sparse: 12 over 3 hours. Few: 4.
        b.expired(
            &[
                vessel("STEADY", 40, 40),
                vessel("SPARSE", 12, 180),
                vessel("FEW", 4, 4),
            ],
            10_000,
        );
        let raised: Vec<String> = b
            .take_changes()
            .iter()
            .map(|c| c.alert().entity.clone())
            .collect();
        assert_eq!(raised, vec!["STEADY".to_string()]);
    }
}
