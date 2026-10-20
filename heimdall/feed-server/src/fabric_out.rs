//! Heimdall → Midgard object fabric: the picture's **significant** items written back
//! to the fabric as typed objects, so Heimdall's view and Forge's curated data meet in
//! one ontology under one policy. [FR-H-2] [FR-H-4] [DD-1] [NFR-SEC-2]
//!
//! Sent through the gateway's `SyncRecords` mesh door (the path Forge uses): each batch
//! is authorised, audited in the enclosure's ledger and checked against the ontology all
//! or nothing, then upserted. What is significant:
//! - every entity with an **open alert**, and the alert itself (`heimdall_alert`, linked
//!   `alert_on` to its track);
//! - the **fixed** items (utility infrastructure, airports, wind farms, platforms,
//!   airspace), sent once a day.
//!
//! A classification alone is not significant: the rules label nearly every item.
//! Moving items are resent when their classification or alerts change, or at most every
//! [`MOVING_RESEND_S`] for their position. An airport track is linked `track_of_airport`
//! to the enclosure's airport object of the same ICAO code, in batches of their own: a
//! link to an airport the enclosure does not hold is dropped and not tried again.
//! Every record carries `synced_at`, the time it was sent, and the fabric's retention
//! windows ([`TRACK_RETENTION_H`], [`ALERT_RETENTION_H`]) run from the latest of it and
//! the item's own times, so what Heimdall stops sending ages out of the fabric while
//! what it still holds is refreshed well inside its window. [SAD_00 §8.5]
//! Every object is marked `internal` for the `operations` purpose, as Forge's live
//! objects are (observations carry no marking of their own yet).

use std::collections::{BTreeMap, BTreeSet};
use std::hash::{Hash, Hasher};

use heimdall_picture::{Attr, Picture, TrackedEntity};
use service_runtime::json::Json;

use crate::alerts::{Alert, AlertBook};

/// The fabric object type of a Heimdall track.
pub const TRACK_TYPE: &str = "heimdall_track";
/// The fabric object type of a Heimdall alert.
pub const ALERT_TYPE: &str = "heimdall_alert";
/// Alert → its track.
pub const ALERT_ON: &str = "alert_on";
/// Airport track → the enclosure's airport object.
pub const TRACK_OF_AIRPORT: &str = "track_of_airport";
/// A moving item's position is resent at most this often.
pub const MOVING_RESEND_S: u64 = 300;
/// Fixed items are resent this often.
pub const STATIC_RESEND_S: u64 = 6 * 3600;
/// An open alert is resent this often, so its `synced_at` stays inside its window.
pub const ALERT_RESEND_S: u64 = 3600;
/// How long the fabric keeps a track after it was last synced or seen.
pub const TRACK_RETENTION_H: u64 = 12;
/// How long the fabric keeps an alert after it was last synced or cleared.
pub const ALERT_RETENTION_H: u64 = 24;
/// The most bytes in one batch (frames are 1 MiB).
pub const BATCH_BYTES: usize = 700 * 1024;
/// The marking and purpose every synced object carries.
const MARKING: &str = "internal";
const PURPOSE: &str = "operations";

fn is_fixed(kind: &str) -> bool {
    kind.starts_with("infra-")
        || matches!(
            kind,
            "land-airport" | "sea-windfarm" | "sea-platform" | "airspace-volume"
        )
}

/// A fabric key from an entity or alert id (no `/`, which separates type and key).
pub fn key(id: &str) -> String {
    id.replace('/', "_")
}

/// What has been sent, so only changes go again.
#[derive(Default)]
pub struct FabricOut {
    sent: BTreeMap<String, (u64, u64)>,
    alerts: BTreeMap<String, (u64, u64)>,
    airports_linked: BTreeSet<String>,
    airports_missing: BTreeSet<String>,
}

/// One round's records: object batches, then airport-link batches.
pub struct Round {
    pub batches: Vec<String>,
    pub link_batches: Vec<(Vec<String>, String)>,
    pub tracks: usize,
    pub alerts: usize,
}

impl FabricOut {
    /// Build this round's batches from the picture and the alert book at `now`.
    pub fn round(&mut self, picture: &Picture, book: &AlertBook, now: u64) -> Round {
        let mut open: BTreeMap<&str, Vec<&Alert>> = BTreeMap::new();
        for a in book.alerts().iter().filter(|a| a.is_open()) {
            open.entry(a.entity.as_str()).or_default().push(a);
        }
        let mut lines: Vec<String> = Vec::new();
        let mut tracks = 0;
        let mut present: BTreeSet<String> = BTreeSet::new();
        let mut airports: Vec<(String, String)> = Vec::new();
        for e in picture.entities() {
            let alerts = open.get(e.id.0.as_str());
            let fixed = is_fixed(&e.kind);
            if alerts.is_none() && !fixed {
                continue;
            }
            let k = key(&e.id.0);
            present.insert(k.clone());
            let mut h = std::collections::hash_map::DefaultHasher::new();
            e.classification.as_ref().map(|c| &c.label).hash(&mut h);
            if let Some(list) = alerts {
                for a in list {
                    a.id.hash(&mut h);
                    a.acknowledged_by.hash(&mut h);
                }
            }
            let sig = h.finish();
            let due = match self.sent.get(&k) {
                None => true,
                Some((at, s)) => {
                    *s != sig
                        || now.saturating_sub(*at)
                            >= if fixed {
                                STATIC_RESEND_S
                            } else {
                                MOVING_RESEND_S
                            }
                }
            };
            if fixed && e.kind == "land-airport" {
                // The enclosure holds GB and Irish airports only.
                if let Some(icao) =
                    text(e, "icao").filter(|i| i.starts_with("EG") || i.starts_with("EI"))
                {
                    if !self.airports_linked.contains(&k) && !self.airports_missing.contains(&icao)
                    {
                        airports.push((k.clone(), icao));
                    }
                }
            }
            if !due {
                continue;
            }
            self.sent.insert(k.clone(), (now, sig));
            lines.push(track_record(e, &k, now).encode());
            tracks += 1;
        }
        // Alerts: new or changed (acknowledged, cleared) ones, linked to their track when
        // the track is in the fabric (sent now or before).
        let mut alert_n = 0;
        for a in book.alerts() {
            let mut h = std::collections::hash_map::DefaultHasher::new();
            (a.cleared_at, &a.acknowledged_by).hash(&mut h);
            let sig = h.finish();
            match self.alerts.get(&a.id) {
                Some((at, s))
                    if *s == sig && (!a.is_open() || now.saturating_sub(*at) < ALERT_RESEND_S) =>
                {
                    continue
                }
                _ => {}
            }
            if !a.is_open() && !self.alerts.contains_key(&a.id) {
                continue; // cleared before it was ever sent: nothing to update
            }
            self.alerts.insert(a.id.clone(), (now, sig));
            lines.push(alert_record(a, now).encode());
            let track = key(&a.entity);
            if present.contains(&track) && self.sent.contains_key(&track) {
                lines.push(
                    Json::obj(vec![
                        ("link", Json::s(ALERT_ON)),
                        ("from", Json::s(&key(&a.id))),
                        ("to", Json::s(&track)),
                    ])
                    .encode(),
                );
            }
            alert_n += 1;
        }
        let link_batches = airports
            .chunks(400)
            .map(|chunk| {
                let ids: Vec<String> = chunk.iter().map(|(k, _)| k.clone()).collect();
                let body = chunk
                    .iter()
                    .filter(|(k, _)| self.sent.contains_key(k))
                    .map(|(k, icao)| {
                        Json::obj(vec![
                            ("link", Json::s(TRACK_OF_AIRPORT)),
                            ("from", Json::s(k)),
                            ("to", Json::s(icao)),
                        ])
                        .encode()
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                (ids, body)
            })
            .filter(|(_, b)| !b.is_empty())
            .collect();
        Round {
            batches: pack(lines),
            link_batches,
            tracks,
            alerts: alert_n,
        }
    }

    /// An airport-link batch was accepted: its tracks are linked.
    pub fn airports_done(&mut self, tracks: &[String]) {
        self.airports_linked.extend(tracks.iter().cloned());
    }

    /// The gateway refused a link to `icao`: never try it again.
    pub fn airport_missing(&mut self, icao: &str) {
        self.airports_missing.insert(icao.to_string());
    }

    /// A batch was refused: forget what it carried, so it goes again next round.
    pub fn forget(&mut self, batch: &str) {
        for line in batch.lines() {
            if let Some(k) = line
                .split("\"key\":\"")
                .nth(1)
                .and_then(|r| r.split('"').next())
            {
                self.sent.remove(k);
                self.alerts.remove(k);
            }
        }
    }
}

/// Join records into batches of at most [`BATCH_BYTES`].
fn pack(lines: Vec<String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut cur = String::new();
    for l in lines {
        if !cur.is_empty() && cur.len() + l.len() + 1 > BATCH_BYTES {
            out.push(std::mem::take(&mut cur));
        }
        if !cur.is_empty() {
            cur.push('\n');
        }
        cur.push_str(&l);
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

fn text(e: &TrackedEntity, k: &str) -> Option<String> {
    match e.latest_attrs.get(k) {
        Some(Attr::Text(t)) if !t.is_empty() => Some(t.clone()),
        _ => None,
    }
}

fn num(e: &TrackedEntity, k: &str) -> Option<f64> {
    match e.latest_attrs.get(k) {
        Some(Attr::Float(f)) if f.is_finite() => Some(*f),
        Some(Attr::Int(i)) => Some(*i as f64),
        _ => None,
    }
}

fn object(kind: &str, k: &str, props: Vec<(&str, Json)>) -> Json {
    Json::obj(vec![
        ("object", Json::s(kind)),
        ("key", Json::s(k)),
        ("marking", Json::s(MARKING)),
        ("purposes", Json::Arr(vec![Json::s(PURPOSE)])),
        (
            "properties",
            Json::obj(
                props
                    .into_iter()
                    .filter(|(_, v)| *v != Json::Null)
                    .collect(),
            ),
        ),
    ])
}

fn track_record(e: &TrackedEntity, k: &str, now: u64) -> Json {
    let clip = |s: String, n: usize| s.chars().take(n).collect::<String>();
    let opt_s = |v: Option<String>| v.map(|s| Json::s(&clip(s, 200))).unwrap_or(Json::Null);
    let opt_f = |v: Option<f64>| v.map(Json::Num).unwrap_or(Json::Null);
    object(
        TRACK_TYPE,
        k,
        vec![
            ("entity_key", Json::s(&e.entity_key)),
            ("kind", Json::s(&e.kind)),
            ("layer", Json::s(&e.layer.0)),
            (
                "name",
                opt_s(text(e, "name").or_else(|| text(e, "callsign"))),
            ),
            (
                "classification",
                opt_s(e.classification.as_ref().map(|c| c.label.clone())),
            ),
            (
                "classification_rationale",
                opt_s(e.classification.as_ref().map(|c| c.rationale.clone())),
            ),
            ("latitude", opt_f(num(e, "lat"))),
            ("longitude", opt_f(num(e, "lon"))),
            ("altitude_m", opt_f(num(e, "alt_m"))),
            ("network", opt_s(text(e, "network"))),
            (
                "feeds",
                Json::s(&clip(
                    e.sources.iter().cloned().collect::<Vec<_>>().join(","),
                    200,
                )),
            ),
            ("observations", Json::Int(e.track.observation_count as i64)),
            ("first_seen", Json::Int(e.track.first_seen as i64)),
            ("last_seen", Json::Int(e.track.last_seen as i64)),
            ("synced_at", Json::Int(now as i64)),
        ],
    )
}

fn alert_record(a: &Alert, now: u64) -> Json {
    let ts = |v: Option<u64>| v.map(|t| Json::Int(t as i64)).unwrap_or(Json::Null);
    object(
        ALERT_TYPE,
        &key(&a.id),
        vec![
            ("alert_id", Json::s(&a.id)),
            ("label", Json::s(&a.label)),
            ("severity", Json::s(a.severity.as_str())),
            (
                "rationale",
                Json::s(&a.rationale.chars().take(300).collect::<String>()),
            ),
            ("subject", Json::s(&a.entity)),
            ("subject_kind", Json::s(&a.kind)),
            ("raised_at", Json::Int(a.raised_at as i64)),
            ("cleared_at", ts(a.cleared_at)),
            (
                "acknowledged_by",
                a.acknowledged_by
                    .as_deref()
                    .map(Json::s)
                    .unwrap_or(Json::Null),
            ),
            ("acknowledged_at", ts(a.acknowledged_at)),
            ("synced_at", Json::Int(now as i64)),
        ],
    )
}

fn retention(after: &[&str], hours: u64) -> Json {
    Json::obj(vec![
        (
            "after",
            Json::Arr(after.iter().map(|a| Json::s(a)).collect()),
        ),
        ("hours", Json::Int(hours as i64)),
    ])
}

/// The ontology additions the sync needs, for an enclosure's `ontology.json`.
pub fn ontology_additions() -> Json {
    let p = |name: &str, ty: &str, required: bool| {
        let mut f = vec![("name", Json::s(name)), ("type", Json::s(ty))];
        if required {
            f.push(("required", Json::Bool(true)));
        }
        Json::obj(f)
    };
    Json::obj(vec![
        (
            "objectTypes",
            Json::Arr(vec![
                Json::obj(vec![
                    ("id", Json::s(TRACK_TYPE)),
                    ("title", Json::s("Heimdall tracked item")),
                    (
                        "properties",
                        Json::Arr(vec![
                            p("entity_key", "text", true),
                            p("kind", "text", true),
                            p("layer", "text", true),
                            p("name", "text", false),
                            p("classification", "text", false),
                            p("classification_rationale", "text", false),
                            p("latitude", "float", false),
                            p("longitude", "float", false),
                            p("altitude_m", "float", false),
                            p("network", "text", false),
                            p("feeds", "text", false),
                            p("observations", "int", false),
                            p("first_seen", "timestamp", false),
                            p("last_seen", "timestamp", false),
                            p("synced_at", "timestamp", false),
                        ]),
                    ),
                    (
                        "retention",
                        retention(&["synced_at", "last_seen"], TRACK_RETENTION_H),
                    ),
                ]),
                Json::obj(vec![
                    ("id", Json::s(ALERT_TYPE)),
                    ("title", Json::s("Heimdall alert")),
                    (
                        "properties",
                        Json::Arr(vec![
                            p("alert_id", "text", true),
                            p("label", "text", true),
                            p("severity", "text", true),
                            p("rationale", "text", false),
                            p("subject", "text", false),
                            p("subject_kind", "text", false),
                            p("raised_at", "timestamp", false),
                            p("cleared_at", "timestamp", false),
                            p("acknowledged_by", "text", false),
                            p("acknowledged_at", "timestamp", false),
                            p("synced_at", "timestamp", false),
                        ]),
                    ),
                    (
                        "retention",
                        retention(&["synced_at", "cleared_at", "raised_at"], ALERT_RETENTION_H),
                    ),
                ]),
            ]),
        ),
        (
            "linkTypes",
            Json::Arr(vec![
                Json::obj(vec![
                    ("id", Json::s(ALERT_ON)),
                    ("name", Json::s("alert on")),
                    ("from", Json::s(ALERT_TYPE)),
                    ("to", Json::s(TRACK_TYPE)),
                    ("cardinality", Json::s("many-to-many")),
                ]),
                Json::obj(vec![
                    ("id", Json::s(TRACK_OF_AIRPORT)),
                    ("name", Json::s("track of airport")),
                    ("from", Json::s(TRACK_TYPE)),
                    ("to", Json::s("airport")),
                    ("cardinality", Json::s("many-to-many")),
                ]),
            ]),
        ),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_carry_no_slash_and_batches_respect_the_size() {
        assert_eq!(key("a/b"), "a_b");
        let lines: Vec<String> = (0..2000)
            .map(|i| format!("{{\"key\":\"K{i}\",\"pad\":\"{}\"}}", "x".repeat(800)))
            .collect();
        let batches = pack(lines);
        assert!(batches.len() > 1);
        assert!(batches.iter().all(|b| b.len() <= BATCH_BYTES));
        assert_eq!(
            batches.iter().map(|b| b.lines().count()).sum::<usize>(),
            2000
        );
    }

    #[test]
    fn a_refused_batch_is_sent_again() {
        let mut out = FabricOut::default();
        out.sent.insert("K1".into(), (1, 1));
        out.forget("{\"object\":\"heimdall_track\",\"key\":\"K1\"}");
        assert!(out.sent.is_empty());
    }
}
