//! The local-suite data guard. `[NFR-PRV-1]` `[AC-C-13]`
//!
//! The demo dataset ships inside an app. It may hold only the release's deterministic
//! mock fixtures; data from a developer's local suite (live feeds, the Mauritius
//! deployment, leak-archive research data) must never reach it. The builder runs
//! [`check`] over the assembled body and refuses to sign on any finding.

use attest::json::Json;

/// Substrings (lower-case) that only local-suite data contains.
pub const DENY_MARKERS: &[&str] = &[
    "icij",
    "offshore leaks",
    "panama papers",
    "pandora papers",
    "paradise papers",
    "mauritius leaks",
    "mauritius",
    "port louis",
    "rodrigues",
    "agalega",
    "agaléga",
    "adsb.lol",
    "adsbexchange",
    "airplanes.live",
    "opensky",
    "aishub",
    "aisstream",
    "marinetraffic",
    "knmi",
    "usgs.gov",
    "emsc-csem",
    "live-feeds",
    "midgard_replay",
];

/// A latitude/longitude box, degrees.
#[derive(Debug, Clone, Copy)]
pub struct Area {
    /// Display name.
    pub name: &'static str,
    /// Southern edge.
    pub lat_min: f64,
    /// Northern edge.
    pub lat_max: f64,
    /// Western edge.
    pub lon_min: f64,
    /// Eastern edge.
    pub lon_max: f64,
}

/// Areas no demo position may fall in.
pub const DENY_AREAS: &[Area] = &[Area {
    name: "Mauritius EEZ",
    lat_min: -23.0,
    lat_max: -9.0,
    lon_min: 55.0,
    lon_max: 65.0,
}];

/// One finding.
#[derive(Debug, Clone, PartialEq)]
pub enum GuardError {
    /// A string holds a denied marker.
    Marker {
        /// Where.
        path: String,
        /// Which marker.
        marker: &'static str,
    },
    /// A position falls in a denied area.
    Area {
        /// Where.
        path: String,
        /// Which area.
        area: &'static str,
    },
}

/// Walk `v`; every finding is reported, none is fatal on its own.
pub fn check(v: &Json) -> Result<(), Vec<GuardError>> {
    let mut found = Vec::new();
    walk(v, "$", &mut found);
    if found.is_empty() {
        Ok(())
    } else {
        Err(found)
    }
}

fn num(pairs: &[(String, Json)], keys: &[&str]) -> Option<f64> {
    pairs
        .iter()
        .find(|(k, _)| keys.contains(&k.as_str()))
        .and_then(|(_, v)| match v {
            Json::Num(t) => t.parse().ok(),
            _ => None,
        })
}

fn walk(v: &Json, path: &str, found: &mut Vec<GuardError>) {
    match v {
        Json::Str(s) => {
            let lower = s.to_lowercase();
            if let Some(m) = DENY_MARKERS.iter().find(|m| lower.contains(*m)) {
                found.push(GuardError::Marker {
                    path: path.to_string(),
                    marker: m,
                });
            }
        }
        Json::Arr(items) => {
            for (i, x) in items.iter().enumerate() {
                walk(x, &format!("{path}[{i}]"), found);
            }
        }
        Json::Obj(pairs) => {
            if let (Some(lat), Some(lon)) = (
                num(pairs, &["lat", "latitude"]),
                num(pairs, &["lon", "longitude"]),
            ) {
                for a in DENY_AREAS {
                    if (a.lat_min..=a.lat_max).contains(&lat)
                        && (a.lon_min..=a.lon_max).contains(&lon)
                    {
                        found.push(GuardError::Area {
                            path: path.to_string(),
                            area: a.name,
                        });
                    }
                }
            }
            for (k, x) in pairs {
                // Keys are data too: a collection named after a live source is refused.
                walk(&Json::Str(k.clone()), &format!("{path}.{k}"), found);
                walk(x, &format!("{path}.{k}"), found);
            }
        }
        Json::Null | Json::Bool(_) | Json::Num(_) => {}
    }
}
