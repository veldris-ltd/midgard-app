//! The browser's read path to Heimdall's common operating picture. [FR-H-1]
//! [NFR-SEC-1] [NFR-AI-1]
//!
//! Heimdall serves its picture only over PQ-mTLS, to workloads whose certificate holds a
//! role its policy lets read it. The gateway is the browser's one door, so it holds such
//! a certificate and asks for the picture on behalf of a signed-in user:
//!
//! 1. the user's request is authenticated, authorised (`Read` on
//!    `/api/v1/heimdall/picture`) and audited in the enclosure ledger, like any read;
//! 2. the gateway then calls `ReadPicture` on `heimdall-feed` as its own identity, which
//!    Heimdall authorises and audits in its own ledger.
//!
//! So both ledgers record the read: who asked, and which workload fetched it.
//!
//! Track history (`GET /api/v1/heimdall/history`) goes the same way, as `ReadHistory`.
//! Heimdall serves it in pages that fit its transport frame; the gateway reads every
//! page over one session and returns the tracks merged, up to [`MAX_HISTORY_PAGES`]
//! pages or [`MAX_HISTORY_BYTES`], after which the result says `"truncated": true`.
//! Each page is one audited `ReadHistory` in Heimdall's ledger. [FR-H-1]

use service_runtime::client;
use service_runtime::json::{parse, Json};
use service_runtime::ServiceIdentity;

use crate::enclosure::EdgeError;

/// The `heimdall-feed` method that returns the picture.
const READ_PICTURE: &str = "ReadPicture";
/// The `heimdall-feed` method that returns a page of track history.
const READ_HISTORY: &str = "ReadHistory";
/// Heimdall's outcome byte for a served request.
const ACCEPTED: u8 = 0;
/// Heimdall's outcome byte for a query it could not make sense of.
const MALFORMED: u8 = 2;
/// The most history pages one gateway request reads.
pub const MAX_HISTORY_PAGES: usize = 50;
/// The most history bytes one gateway request reads.
pub const MAX_HISTORY_BYTES: usize = 40 * 1024 * 1024;
/// The window a history request covers when it names no `from`, in seconds.
pub const DEFAULT_HISTORY_WINDOW: u64 = 3600;

/// A history request: a window in unix seconds, optional layer and entity filters, and
/// the least spacing between a track's points (0: all).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryParams {
    /// Earliest point, inclusive.
    pub from: u64,
    /// Latest point, inclusive.
    pub to: u64,
    /// Only this layer id.
    pub layer: Option<String>,
    /// Only this entity id.
    pub entity: Option<String>,
    /// Least seconds between returned points of one track.
    pub step: u64,
}

impl HistoryParams {
    /// Parse the query string of `GET /api/v1/heimdall/history`: `from`, `to` (unix
    /// seconds; `to` defaults to `now` and `from` to an hour before `to`), `layer`,
    /// `entity` and `step`. A value that is not a whole number, or `to` before `from`,
    /// is a 400.
    pub fn from_query(query: &str, now: u64) -> Result<Self, EdgeError> {
        let mut values: Vec<(String, String)> = Vec::new();
        for pair in query.split('&').filter(|p| !p.is_empty()) {
            let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
            values.push((percent_decode(k)?, percent_decode(v)?));
        }
        let get = |key: &str| {
            values
                .iter()
                .find(|(k, _)| k == key)
                .map(|(_, v)| v.as_str())
                .filter(|v| !v.is_empty())
        };
        let seconds = |key: &str| -> Result<Option<u64>, EdgeError> {
            get(key)
                .map(|v| {
                    v.parse::<u64>().map_err(|_| {
                        EdgeError::bad_request(&format!(
                            "{key} must be a whole number of seconds, not '{v}'"
                        ))
                    })
                })
                .transpose()
        };
        let to = seconds("to")?.unwrap_or(now);
        let from = seconds("from")?.unwrap_or(to.saturating_sub(DEFAULT_HISTORY_WINDOW));
        if to < from {
            return Err(EdgeError::bad_request(&format!(
                "to ({to}) is before from ({from})"
            )));
        }
        Ok(Self {
            from,
            to,
            layer: get("layer").map(str::to_string),
            entity: get("entity").map(str::to_string),
            step: seconds("step")?.unwrap_or(0),
        })
    }

    /// The `ReadHistory` query for the page after `after`.
    fn request(&self, after: &Json) -> Vec<u8> {
        let opt = |v: &Option<String>| v.as_deref().map_or(Json::Null, Json::s);
        Json::obj(vec![
            ("from", count(self.from)),
            ("to", count(self.to)),
            ("layer", opt(&self.layer)),
            ("entity", opt(&self.entity)),
            ("step", count(self.step)),
            ("after", after.clone()),
        ])
        .encode()
        .into_bytes()
    }
}

fn count(v: u64) -> Json {
    Json::Int(i64::try_from(v).unwrap_or(i64::MAX))
}

/// Decode `%XX` escapes and `+` in a query component; a bad escape is a 400.
fn percent_decode(s: &str) -> Result<String, EdgeError> {
    let bad = || EdgeError::bad_request("the query string has a malformed escape");
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'%' => {
                let hex = b.get(i + 1..i + 3).ok_or_else(bad)?;
                let hex = std::str::from_utf8(hex).map_err(|_| bad())?;
                out.push(u8::from_str_radix(hex, 16).map_err(|_| bad())?);
                i += 3;
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8(out).map_err(|_| EdgeError::bad_request("the query string is not UTF-8"))
}

/// Tracks merged across history pages, in the order Heimdall serves them.
#[derive(Default)]
struct MergedTracks {
    order: Vec<String>,
    tracks: std::collections::BTreeMap<String, (Json, Json, Vec<Json>)>,
}

impl MergedTracks {
    fn add(&mut self, page: &Json) -> Result<(), EdgeError> {
        let bad = || EdgeError::new(502, "Heimdall's history page is malformed");
        let Some(Json::Arr(tracks)) = page.get("tracks") else {
            return Err(bad());
        };
        for track in tracks {
            let id = track.str_field("id").ok_or_else(bad)?.to_string();
            let Some(Json::Arr(points)) = track.get("points") else {
                return Err(bad());
            };
            match self.tracks.get_mut(&id) {
                Some((_, _, all)) => all.extend(points.iter().cloned()),
                None => {
                    let kind = track.get("kind").cloned().unwrap_or(Json::Null);
                    let layer = track.get("layer").cloned().unwrap_or(Json::Null);
                    self.order.push(id.clone());
                    self.tracks.insert(id, (kind, layer, points.clone()));
                }
            }
        }
        Ok(())
    }

    fn into_json(mut self) -> Json {
        Json::Arr(
            self.order
                .iter()
                .filter_map(|id| {
                    self.tracks.remove(id).map(|(kind, layer, points)| {
                        Json::obj(vec![
                            ("id", Json::s(id)),
                            ("kind", kind),
                            ("layer", layer),
                            ("points", Json::Arr(points)),
                        ])
                    })
                })
                .collect(),
        )
    }
}

/// Make sense of one `ReadHistory` response: the page, or the error the browser gets.
/// A query Heimdall finds malformed (a window longer than it keeps history for) is the
/// caller's to fix, a 400; any other refusal is a 502.
fn history_page(response: &[u8]) -> Result<Json, EdgeError> {
    let (code, body) = response
        .split_first()
        .ok_or_else(|| EdgeError::new(502, "Heimdall sent an empty response"))?;
    let body = String::from_utf8_lossy(body);
    match *code {
        ACCEPTED => {
            parse(&body).map_err(|e| EdgeError::new(502, &format!("Heimdall's history: {e}")))
        }
        MALFORMED => Err(EdgeError::bad_request(&format!(
            "Heimdall refused the query: {body}"
        ))),
        _ => Err(EdgeError::new(
            502,
            &format!("Heimdall refused the read: {body}"),
        )),
    }
}

/// Where the picture is and who the gateway is when it asks for it.
pub struct HeimdallBridge {
    identity: ServiceIdentity,
    addr: String,
    purpose: String,
}

impl HeimdallBridge {
    /// Read the picture from `heimdall-feed` at `addr` as `identity`, declaring `purpose`
    /// (the one purpose Heimdall's channel opens for).
    pub fn new(identity: ServiceIdentity, addr: &str, purpose: &str) -> Self {
        Self {
            identity,
            addr: addr.to_string(),
            purpose: purpose.to_string(),
        }
    }

    /// The picture as Heimdall serves it: `{"layers": [..], "entities": [..]}`. Heimdall
    /// being unreachable, or refusing the read, is a 502: the gateway is a client there.
    pub fn picture(&self) -> Result<Json, EdgeError> {
        let response =
            client::call(&self.identity, &self.addr, &self.purpose, READ_PICTURE, &[])
                .map_err(|e| EdgeError::new(502, &format!("Heimdall is unreachable: {e}")))?;
        let (code, body) = response
            .split_first()
            .ok_or_else(|| EdgeError::new(502, "Heimdall sent an empty response"))?;
        let body = String::from_utf8_lossy(body);
        if *code != ACCEPTED {
            return Err(EdgeError::new(
                502,
                &format!("Heimdall refused the read: {body}"),
            ));
        }
        parse(&body).map_err(|e| EdgeError::new(502, &format!("Heimdall's picture: {e}")))
    }

    /// Track history as `{"from", "to", "tracks": [{"id", "kind", "layer", "points":
    /// [[t, lat, lon, alt_m|null], ..]}], "truncated"}`: every page Heimdall serves for
    /// the query, read over one session and merged, up to [`MAX_HISTORY_PAGES`] pages or
    /// [`MAX_HISTORY_BYTES`]. Heimdall being unreachable or refusing the read is a 502.
    pub fn history(&self, params: &HistoryParams) -> Result<Json, EdgeError> {
        let unreachable = |e: String| EdgeError::new(502, &format!("Heimdall is unreachable: {e}"));
        let mut session = client::Session::open(&self.identity, &self.addr, &self.purpose)
            .map_err(unreachable)?;
        let mut merged = MergedTracks::default();
        let mut after = Json::Null;
        let mut bytes = 0usize;
        let mut truncated = false;
        for page_no in 0.. {
            if page_no == MAX_HISTORY_PAGES || bytes >= MAX_HISTORY_BYTES {
                truncated = true;
                break;
            }
            let response = session
                .call(READ_HISTORY, &params.request(&after))
                .map_err(unreachable)?;
            bytes += response.len();
            let page = history_page(&response)?;
            merged.add(&page)?;
            match page.get("next") {
                None | Some(Json::Null) => break,
                Some(next @ Json::Str(_)) => after = next.clone(),
                Some(_) => {
                    return Err(EdgeError::new(
                        502,
                        "Heimdall's history cursor is malformed",
                    ))
                }
            }
        }
        Ok(Json::obj(vec![
            ("from", count(params.from)),
            ("to", count(params.to)),
            ("tracks", merged.into_json()),
            ("truncated", Json::Bool(truncated)),
        ]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_query_defaults_decodes_and_refuses() {
        let p = HistoryParams::from_query("", 10_000).unwrap();
        assert_eq!((p.from, p.to, p.step), (10_000 - 3600, 10_000, 0));
        assert_eq!((p.layer, p.entity), (None, None));
        let p = HistoryParams::from_query("to=500&from=100&layer=air&entity=AC%2D1+x&step=30", 0)
            .unwrap();
        assert_eq!((p.from, p.to, p.step), (100, 500, 30));
        assert_eq!(p.layer.as_deref(), Some("air"));
        assert_eq!(p.entity.as_deref(), Some("AC-1 x"));
        let p = HistoryParams::from_query("to=100&layer=", 0).unwrap();
        assert_eq!(
            (p.from, p.layer),
            (0, None),
            "from saturates; empty is absent"
        );
        for bad in [
            "from=abc",
            "to=-1",
            "from=200&to=100",
            "step=1.5",
            "entity=%zz",
            "entity=%4",
        ] {
            let e = HistoryParams::from_query(bad, 1_000).unwrap_err();
            assert_eq!(e.status, 400, "{bad}");
        }
    }

    #[test]
    fn history_pages_merge_and_refusals_map_to_status() {
        let mut merged = MergedTracks::default();
        let page = |json: &str| {
            let mut bytes = vec![ACCEPTED];
            bytes.extend_from_slice(json.as_bytes());
            history_page(&bytes).unwrap()
        };
        merged
            .add(&page(r#"{"tracks":[{"id":"A","kind":"air-track","layer":"air","points":[[1,2,3,null]]}],"next":"1:A"}"#))
            .unwrap();
        merged
            .add(&page(r#"{"tracks":[{"id":"A","kind":"air-track","layer":"air","points":[[2,2,3,4]]},{"id":"B","kind":"k","layer":"l","points":[[5,0,0,null]]}],"next":null}"#))
            .unwrap();
        assert_eq!(
            merged.into_json().encode(),
            Json::Arr(vec![
                parse(r#"{"id":"A","kind":"air-track","layer":"air","points":[[1,2,3,null],[2,2,3,4]]}"#).unwrap(),
                parse(r#"{"id":"B","kind":"k","layer":"l","points":[[5,0,0,null]]}"#).unwrap(),
            ])
            .encode()
        );
        assert_eq!(history_page(&[]).unwrap_err().status, 502);
        assert_eq!(history_page(b"\x01denied").unwrap_err().status, 502);
        assert_eq!(history_page(b"\x03failed").unwrap_err().status, 502);
        assert_eq!(
            history_page(b"\x02window too long").unwrap_err().status,
            400
        );
        assert_eq!(history_page(b"\x00not json").unwrap_err().status, 502);
        assert!(MergedTracks::default().add(&parse("{}").unwrap()).is_err());
    }
}
