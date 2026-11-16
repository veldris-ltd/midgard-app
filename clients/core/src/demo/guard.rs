//! The local-suite data guard. `[NFR-PRV-1]` `[AC-C-13]`
//!
//! The demo dataset ships inside an app. It may hold only the release's deterministic
//! mock fixtures; data from a developer's local suite (live feeds, the Mauritius
//! deployment, leak-archive research data) must never reach it. The builder runs
//! [`check`] over the assembled body and refuses to sign on any finding.
//!
//! Text is compared after [`normalise`]: accents and full-width forms folded to
//! ASCII, invisible format characters dropped, case folded, and runs of whitespace,
//! `_`, `-` and `.` read as one space, so `Port_Louis`, `ＭＡＵＲＩＴＩＵＳ` and a
//! decomposed `Agaléga` are all found, and markers are also matched with every space
//! removed (`PortLouis`). Characters are allow-listed: a string holding anything but
//! ASCII, the folded Latin letters and a short list of symbols (another script, a
//! combining mark, a variation selector, a tag, an invisible separator) is refused
//! outright.
//!
//! What it does not catch: data encoded so it no longer reads as text (base64, hex,
//! compressed blobs). Task A7's builder covers that by accepting only inputs whose
//! blob ids match the tagged mocks. Short markers such as `rodrigues` or `knmi` can
//! match an innocent word; that fails safe on purpose (rename the synthetic value).
//! Likewise a bare pair (an array or a string of two numbers) is tried both ways
//! round, so an Irish `[lon, lat]` such as `[-6.2, 53.4]` reads as a point in the
//! Mauritius box and is refused: the demo keys its positions as `lat`/`lon`.

use attest::json::Json;

/// Substrings that only local-suite data contains, compared after [`normalise`].
///
/// This list mirrors the local suite's fetch hosts and job names, so it changes when
/// the local suite gains a source: leak archives, the live area, the live-feed and
/// open-data hosts the suite fetches from, and its replay and live-loop switches.
pub const DENY_MARKERS: &[&str] = &[
    // Leak archives.
    "icij",
    "offshore leaks",
    "panama papers",
    "pandora papers",
    "paradise papers",
    "mauritius leaks",
    // The local suite's live area.
    "mauritius",
    "port louis",
    "rodrigues",
    "agalega",
    "agaléga",
    "ceb.mu",
    "govmu.org",
    // Live-feed and open-data hosts the local suite fetches from.
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
    "seismicportal.eu",
    "api.tfl.gov.uk",
    "data.elexon.co.uk",
    "api.carbonintensity.org.uk",
    "environment.data.gov.uk",
    "data.sensor.community",
    "open-meteo.com",
    "aviationweather.gov",
    "gdacs.org",
    "eonet.gsfc.nasa.gov",
    "coralreefwatch.noaa.gov",
    "services.swpc.noaa.gov",
    "ncei.noaa.gov",
    "erddap.ifremer.fr",
    "euro-argo.eu",
    "earth-search.aws.element84.com",
    "rapidmapping.emergency.copernicus.eu",
    "worldweather.wmo.int",
    "cap-sources.s3.amazonaws.com",
    // The local suite's switches and loops.
    "live-feeds",
    "midgard_replay",
    "fetch_live",
    "live_loop",
];

/// Prefixes refused only at the start of a token (the local suite's `mu_` job and
/// type names), compared case-folded with separators kept as written.
pub const DENY_TOKEN_PREFIXES: &[&str] = &["mu_"];

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
pub const DENY_AREAS: &[Area] = &[
    Area {
        name: "Mauritius EEZ",
        lat_min: -25.0,
        lat_max: -6.0,
        lon_min: 52.0,
        lon_max: 68.5,
    },
    Area {
        name: "Chagos Archipelago",
        lat_min: -9.0,
        lat_max: -3.5,
        lon_min: 69.0,
        lon_max: 74.5,
    },
];

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
    /// A string holds a character outside the allowed set: ASCII, the folded Latin
    /// letters and a short list of symbols (degree, dashes, arrows, signs).
    ForeignChar {
        /// Where.
        path: String,
        /// The first such code point.
        code: u32,
    },
}

/// Walk `v`; every finding is reported, none is fatal on its own.
#[must_use = "a finding must stop the build"]
pub fn check(v: &Json) -> Result<(), Vec<GuardError>> {
    let markers: Vec<(String, &'static str)> =
        DENY_MARKERS.iter().map(|m| (squash(m), *m)).collect();
    let mut found = Vec::new();
    walk(v, "$", &markers, &mut found);
    if found.is_empty() {
        Ok(())
    } else {
        Err(found)
    }
}

/// Fold `s` for marker matching: drop invisible format characters and combining
/// marks, map full-width ASCII and Latin-1/Latin Extended-A letters to ASCII,
/// lower-case, and read each run of whitespace, `_`, `-`, `.` and the typographic
/// dashes, dots and bullets as one space.
pub fn normalise(s: &str) -> String {
    fold(s, true)
}

/// [`normalise`] with the spaces removed too, so `PortLouis` and `p o r t louis`
/// both read `portlouis`.
fn squash(s: &str) -> String {
    normalise(s).replace(' ', "")
}

/// Whether `c` may appear in the demo dataset at all: ASCII, the Latin letters
/// [`FOLD`] maps, and the symbols the release fixtures use (degree to fractions,
/// multiplication sign, general punctuation, arrows, minus, comparison signs,
/// miscellaneous symbols and dingbats). Everything else (other scripts, variation
/// selectors, combining marks, tags, invisible separators) is refused.
fn is_allowed(c: char) -> bool {
    let cp = c as u32;
    c.is_ascii()
        || FOLD.iter().any(|(lo, hi, _)| (*lo..=*hi).contains(&cp))
        || matches!(cp,
            0xb0..=0xbf | 0xd7 | 0x2010..=0x2027 | 0x2190..=0x21ff | 0x2212
            | 0x2260..=0x2265 | 0x2600..=0x27bf)
}

fn is_separator(c: char) -> bool {
    c.is_whitespace()
        || matches!(c, '_' | '-' | '.' | '\u{b7}' | '\u{2010}'..='\u{2015}' | '\u{2022}' | '\u{2024}'..='\u{2027}')
}

fn is_hidden(c: char) -> bool {
    matches!(c as u32,
        0x0300..=0x036f | 0x00ad | 0x200b..=0x200f | 0x202a..=0x202e
        | 0x2060..=0x2064 | 0x2066..=0x2069 | 0xfeff)
}

/// Latin-1 Supplement and Latin Extended-A letters, by code-point range, folded.
const FOLD: &[(u32, u32, &str)] = &[
    (0xaa, 0xaa, "a"),
    (0xb2, 0xb2, "2"),
    (0xb3, 0xb3, "3"),
    (0xb5, 0xb5, "u"),
    (0xb9, 0xb9, "1"),
    (0xba, 0xba, "o"),
    (0xc0, 0xc5, "a"),
    (0xc6, 0xc6, "ae"),
    (0xc7, 0xc7, "c"),
    (0xc8, 0xcb, "e"),
    (0xcc, 0xcf, "i"),
    (0xd0, 0xd0, "d"),
    (0xd1, 0xd1, "n"),
    (0xd2, 0xd6, "o"),
    (0xd8, 0xd8, "o"),
    (0xd9, 0xdc, "u"),
    (0xdd, 0xdd, "y"),
    (0xde, 0xde, "th"),
    (0xdf, 0xdf, "ss"),
    (0xe0, 0xe5, "a"),
    (0xe6, 0xe6, "ae"),
    (0xe7, 0xe7, "c"),
    (0xe8, 0xeb, "e"),
    (0xec, 0xef, "i"),
    (0xf0, 0xf0, "d"),
    (0xf1, 0xf1, "n"),
    (0xf2, 0xf6, "o"),
    (0xf8, 0xf8, "o"),
    (0xf9, 0xfc, "u"),
    (0xfd, 0xfd, "y"),
    (0xfe, 0xfe, "th"),
    (0xff, 0xff, "y"),
    (0x100, 0x105, "a"),
    (0x106, 0x10d, "c"),
    (0x10e, 0x111, "d"),
    (0x112, 0x11b, "e"),
    (0x11c, 0x123, "g"),
    (0x124, 0x127, "h"),
    (0x128, 0x131, "i"),
    (0x132, 0x133, "ij"),
    (0x134, 0x135, "j"),
    (0x136, 0x138, "k"),
    (0x139, 0x142, "l"),
    (0x143, 0x14b, "n"),
    (0x14c, 0x151, "o"),
    (0x152, 0x153, "oe"),
    (0x154, 0x159, "r"),
    (0x15a, 0x161, "s"),
    (0x162, 0x167, "t"),
    (0x168, 0x173, "u"),
    (0x174, 0x175, "w"),
    (0x176, 0x178, "y"),
    (0x179, 0x17e, "z"),
    (0x17f, 0x17f, "s"),
];

fn fold(s: &str, collapse: bool) -> String {
    let mut out = String::with_capacity(s.len());
    let mut gap = false;
    for c in s.chars().filter(|c| !is_hidden(*c)) {
        let c = match c as u32 {
            0xff01..=0xff5e => char::from_u32(c as u32 - 0xfee0).unwrap_or(c),
            0x2212 => '-',
            _ => c,
        };
        if collapse && is_separator(c) {
            gap = true;
            continue;
        }
        if gap {
            out.push(' ');
            gap = false;
        }
        let cp = c as u32;
        if c.is_ascii() {
            out.push(c.to_ascii_lowercase());
            continue;
        }
        match FOLD.iter().find(|(lo, hi, _)| (*lo..=*hi).contains(&cp)) {
            Some((_, _, ascii)) => out.push_str(ascii),
            None => out.extend(c.to_lowercase()),
        }
    }
    out
}

fn marker_in(s: &str, markers: &[(String, &'static str)]) -> Option<&'static str> {
    let text = squash(s);
    if let Some((_, m)) = markers.iter().find(|(n, _)| text.contains(n.as_str())) {
        return Some(m);
    }
    let raw = fold(s, false);
    DENY_TOKEN_PREFIXES.iter().copied().find(|p| {
        raw.match_indices(p).any(|(i, _)| {
            raw[..i]
                .chars()
                .next_back()
                .map_or(true, |c| !c.is_alphanumeric())
        })
    })
}

/// A number as JSON gives it, or a string holding only a number.
fn as_number(v: &Json) -> Option<f64> {
    let t = match v {
        Json::Num(t) => t.as_str(),
        Json::Str(t) => t.trim(),
        _ => return None,
    };
    t.parse::<f64>().ok().filter(|x| x.is_finite())
}

fn in_areas(lat: f64, lon: f64, path: &str, found: &mut Vec<GuardError>) {
    for a in DENY_AREAS {
        if (a.lat_min..=a.lat_max).contains(&lat) && (a.lon_min..=a.lon_max).contains(&lon) {
            found.push(GuardError::Area {
                path: path.to_string(),
                area: a.name,
            });
        }
    }
}

/// A bare pair, order unknown: `[lat, lon]` and `[lon, lat]` are both tried.
fn pair_in_areas(a: f64, b: f64, path: &str, found: &mut Vec<GuardError>) {
    let before = found.len();
    in_areas(a, b, path, found);
    if found.len() == before {
        in_areas(b, a, path, found);
    }
}

/// Every number under a key whose lower case is one of `keys`.
fn keyed(pairs: &[(String, Json)], keys: &[&str]) -> Vec<f64> {
    pairs
        .iter()
        .filter(|(k, _)| keys.contains(&k.to_lowercase().as_str()))
        .filter_map(|(_, v)| as_number(v))
        .collect()
}

/// Decimal numbers written in `s`, each with the text between it and the one before.
fn numbers_in(s: &str) -> Vec<(f64, String)> {
    let cs: Vec<char> = fold(s, false).chars().collect();
    let mut out = Vec::new();
    let mut gap = String::new();
    let mut i = 0;
    while i < cs.len() {
        let prev_ok = i == 0 || !(cs[i - 1].is_alphanumeric() || cs[i - 1] == '.');
        let signed = matches!(cs[i], '-' | '+') && cs.get(i + 1).is_some_and(char::is_ascii_digit);
        if prev_ok && (signed || cs[i].is_ascii_digit()) {
            let start = i;
            i += 1;
            while i < cs.len() && cs[i].is_ascii_digit() {
                i += 1;
            }
            if i + 1 < cs.len() && cs[i] == '.' && cs[i + 1].is_ascii_digit() {
                i += 1;
                while i < cs.len() && cs[i].is_ascii_digit() {
                    i += 1;
                }
            }
            // An exponent: e, an optional sign, digits.
            if i + 1 < cs.len() && matches!(cs[i], 'e' | 'E') {
                let mut j = i + 1;
                if matches!(cs[j], '-' | '+') {
                    j += 1;
                }
                if cs.get(j).is_some_and(char::is_ascii_digit) {
                    i = j;
                    while i < cs.len() && cs[i].is_ascii_digit() {
                        i += 1;
                    }
                }
            }
            let text: String = cs[start..i].iter().collect();
            if let Ok(x) = text.parse::<f64>() {
                out.push((x, std::mem::take(&mut gap)));
            }
            continue;
        }
        gap.push(cs[i]);
        i += 1;
    }
    out
}

fn key_path(path: &str, k: &str) -> String {
    if k.contains(['.', '[', ']']) {
        format!("{path}[{k:?}]")
    } else {
        format!("{path}.{k}")
    }
}

fn walk(v: &Json, path: &str, markers: &[(String, &'static str)], found: &mut Vec<GuardError>) {
    match v {
        Json::Str(s) => {
            if let Some(c) = s.chars().find(|c| !is_allowed(*c)) {
                found.push(GuardError::ForeignChar {
                    path: path.to_string(),
                    code: c as u32,
                });
            }
            if let Some(m) = marker_in(s, markers) {
                found.push(GuardError::Marker {
                    path: path.to_string(),
                    marker: m,
                });
            }
            // "57.5 -20.16", "POINT(57.5 -20.16)", "lat=-20.16 lon=57.5": two numbers
            // at most eight characters apart.
            for w in numbers_in(s).windows(2) {
                if (1..=8).contains(&w[1].1.chars().count()) {
                    pair_in_areas(w[0].0, w[1].0, path, found);
                }
            }
        }
        Json::Arr(items) => {
            let nums: Vec<f64> = items.iter().filter_map(as_number).collect();
            // [lon, lat], [lon, lat, alt], or a run of pairs [lon, lat, lon, lat, ...].
            if nums.len() == items.len() {
                if nums.len() == 3 {
                    pair_in_areas(nums[0], nums[1], path, found);
                } else if nums.len() % 2 == 0 {
                    for p in nums.chunks(2) {
                        pair_in_areas(p[0], p[1], path, found);
                    }
                }
            }
            for (i, x) in items.iter().enumerate() {
                walk(x, &format!("{path}[{i}]"), markers, found);
            }
        }
        Json::Obj(pairs) => {
            // Every latitude-like value against every longitude-like one. Read one
            // way round only: swapped, the synthetic Heimdall hour (keyed, lat 51 to
            // 56, lon -10 to -2) would fall in the Mauritius box.
            let lats = keyed(pairs, &["lat", "latitude", "latdeg", "lat_deg", "y"]);
            let lons = keyed(
                pairs,
                &["lon", "lng", "long", "longitude", "londeg", "lon_deg", "x"],
            );
            for lat in &lats {
                for lon in &lons {
                    in_areas(*lat, *lon, path, found);
                }
            }
            for (k, x) in pairs {
                let p = key_path(path, k);
                // Keys are data too: a collection named after a live source is refused.
                walk(&Json::Str(k.clone()), &p, markers, found);
                walk(x, &p, markers, found);
            }
        }
        Json::Null | Json::Bool(_) | Json::Num(_) => {}
    }
}
