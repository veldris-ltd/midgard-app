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
//! Heimdall serves the picture in pages that fit its transport frame; the gateway reads
//! every page over one session and returns `{"layers", "entities"}` merged (layers from
//! the first page), up to [`MAX_PICTURE_PAGES`] pages or [`MAX_PICTURE_BYTES`], after
//! which the result also says `"truncated": true`. Each page is one audited
//! `ReadPicture` in Heimdall's ledger. [FR-H-1]
//!
//! Track history (`GET /api/v1/heimdall/history`) goes the same way, as `ReadHistory`.
//! Heimdall serves it in pages that fit its transport frame; the gateway reads every
//! page over one session and returns the tracks merged, up to [`MAX_HISTORY_PAGES`]
//! pages or [`MAX_HISTORY_BYTES`], after which the result says `"truncated": true`.
//! Each page is one audited `ReadHistory` in Heimdall's ledger. [FR-H-1]
//!
//! Alerts (`GET /api/v1/heimdall/alerts?state=open|all`) are read the same way, as
//! `ReadAlerts`. Acknowledging one (`POST /api/v1/heimdall/alerts/{id}/ack`) is a write:
//! the gateway authorises it as `Apply` and audits it under the signed-in user, then
//! calls `AcknowledgeAlert` with that user's name as `by`, which Heimdall authorises and
//! audits again, recording `by`. An unknown alert is a 404. [FR-H-4] [NFR-SEC-1]

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
/// Heimdall's outcome byte for an alert it does not hold.
const NOT_FOUND: u8 = 4;
/// The `heimdall-feed` method that returns the alerts.
const READ_ALERTS: &str = "ReadAlerts";
/// The `heimdall-feed` method that acknowledges an alert.
const ACKNOWLEDGE_ALERT: &str = "AcknowledgeAlert";
/// The path alerts are read at; an acknowledgement is `{ALERTS_PATH}/{id}/ack`.
pub const ALERTS_PATH: &str = "/api/v1/heimdall/alerts";

/// Parse the query string of `GET /api/v1/heimdall/alerts`: `state` is `open` (the
/// default) or `all`. Returns whether only open alerts are wanted; anything else is a
/// 400. [FR-H-4]
pub fn alerts_state(query: &str) -> Result<bool, EdgeError> {
    let mut open_only = true;
    for pair in query.split('&').filter(|p| !p.is_empty()) {
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        let (k, v) = (percent_decode(k)?, percent_decode(v)?);
        match (k.as_str(), v.as_str()) {
            ("state", "open") | ("state", "") => open_only = true,
            ("state", "all") => open_only = false,
            ("state", other) => {
                return Err(EdgeError::bad_request(&format!(
                    "state must be open or all, not '{other}'"
                )))
            }
            _ => {}
        }
    }
    Ok(open_only)
}

/// The alert id an acknowledgement path names: `Some` for any path of the form
/// `/api/v1/heimdall/alerts/{id}/ack`, with the id or a 400 for one that is not an
/// alert id (1-64 letters, digits, `-` or `_`, after percent-decoding). [FR-H-4]
pub fn ack_target(path: &str) -> Option<Result<String, EdgeError>> {
    let rest = path.strip_prefix(ALERTS_PATH)?.strip_prefix('/')?;
    let raw = rest.strip_suffix("/ack")?;
    Some(percent_decode(raw).and_then(|id| {
        let ok = !id.is_empty()
            && id.len() <= 64
            && id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
        if ok {
            Ok(id)
        } else {
            Err(EdgeError::bad_request(&format!(
                "'{raw}' is not an alert id"
            )))
        }
    }))
}

/// Make sense of a `ReadAlerts` or `AcknowledgeAlert` response: its JSON, or the error
/// the browser gets (unknown alert 404, malformed 400, anything else 502).
fn alerts_response(response: &[u8]) -> Result<Json, EdgeError> {
    let (code, body) = response
        .split_first()
        .ok_or_else(|| EdgeError::new(502, "Heimdall sent an empty response"))?;
    let body = String::from_utf8_lossy(body);
    match *code {
        ACCEPTED => {
            parse(&body).map_err(|e| EdgeError::new(502, &format!("Heimdall's alerts: {e}")))
        }
        NOT_FOUND => Err(EdgeError::new(404, &body)),
        MALFORMED => Err(EdgeError::bad_request(&format!(
            "Heimdall refused the request: {body}"
        ))),
        _ => Err(EdgeError::new(
            502,
            &format!("Heimdall refused the request: {body}"),
        )),
    }
}
/// The most history pages one gateway request reads.
pub const MAX_HISTORY_PAGES: usize = 50;
/// The most history bytes one gateway request reads.
pub const MAX_HISTORY_BYTES: usize = 40 * 1024 * 1024;
/// The most picture pages one gateway request reads.
pub const MAX_PICTURE_PAGES: usize = 50;
/// The most picture bytes one gateway request reads.
pub const MAX_PICTURE_BYTES: usize = 40 * 1024 * 1024;

/// Picture pages merged: the first page's layers, then every page's entities.
#[derive(Default)]
struct MergedPicture {
    layers: Option<Json>,
    entities: Vec<Json>,
}

impl MergedPicture {
    /// Add one page; returns its `next` cursor (`Json::Null` at the end).
    fn add(&mut self, page: &Json) -> Result<Json, EdgeError> {
        let bad = || EdgeError::new(502, "Heimdall's picture page is malformed");
        let Some(Json::Arr(entities)) = page.get("entities") else {
            return Err(bad());
        };
        if self.layers.is_none() {
            match page.get("layers") {
                Some(layers @ Json::Arr(_)) => self.layers = Some(layers.clone()),
                _ => return Err(bad()),
            }
        }
        self.entities.extend(entities.iter().cloned());
        match page.get("next") {
            None | Some(Json::Null) => Ok(Json::Null),
            Some(next @ Json::Str(_)) => Ok(next.clone()),
            Some(_) => Err(EdgeError::new(
                502,
                "Heimdall's picture cursor is malformed",
            )),
        }
    }

    fn into_json(self, truncated: bool) -> Json {
        let mut pairs = vec![
            ("layers", self.layers.unwrap_or(Json::Arr(Vec::new()))),
            ("entities", Json::Arr(self.entities)),
        ];
        if truncated {
            pairs.push(("truncated", Json::Bool(true)));
        }
        Json::obj(pairs)
    }
}

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

/// Make sense of one `ReadPicture` response: the page, or a 502.
fn picture_page(response: &[u8]) -> Result<Json, EdgeError> {
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

    /// The picture as `{"layers": [..], "entities": [..]}`: every page Heimdall serves,
    /// read over one session and merged, up to [`MAX_PICTURE_PAGES`] pages or
    /// [`MAX_PICTURE_BYTES`], past which `"truncated": true` is added. Heimdall being
    /// unreachable, or refusing the read, is a 502: the gateway is a client there.
    /// [FR-H-1]
    pub fn picture(&self) -> Result<Json, EdgeError> {
        let unreachable = |e: String| EdgeError::new(502, &format!("Heimdall is unreachable: {e}"));
        let mut session = client::Session::open(&self.identity, &self.addr, &self.purpose)
            .map_err(unreachable)?;
        let mut merged = MergedPicture::default();
        let mut request = Vec::new();
        let mut bytes = 0usize;
        for _ in 0..MAX_PICTURE_PAGES {
            if bytes >= MAX_PICTURE_BYTES {
                break;
            }
            let response = session.call(READ_PICTURE, &request).map_err(unreachable)?;
            bytes += response.len();
            let next = merged.add(&picture_page(&response)?)?;
            if next == Json::Null {
                return Ok(merged.into_json(false));
            }
            request = Json::obj(vec![("after", next)]).encode().into_bytes();
        }
        Ok(merged.into_json(true))
    }

    /// The alerts as `{"alerts": [..]}`, newest first: only open ones, or all. Heimdall
    /// being unreachable or refusing the read is a 502. [FR-H-4]
    pub fn alerts(&self, open_only: bool) -> Result<Json, EdgeError> {
        let request = Json::obj(vec![(
            "state",
            Json::s(if open_only { "open" } else { "all" }),
        )]);
        let response = client::call(
            &self.identity,
            &self.addr,
            &self.purpose,
            READ_ALERTS,
            request.encode().as_bytes(),
        )
        .map_err(|e| EdgeError::new(502, &format!("Heimdall is unreachable: {e}")))?;
        let body = alerts_response(&response)?;
        match body.get("alerts") {
            Some(Json::Arr(items)) => Ok(Json::obj(vec![("alerts", Json::Arr(items.clone()))])),
            _ => Err(EdgeError::new(502, "Heimdall's alerts are malformed")),
        }
    }

    /// Acknowledge alert `id` as `by` (the signed-in user's name): the updated alert.
    /// An unknown alert is a 404; Heimdall unreachable or refusing is a 502. [FR-H-4]
    pub fn acknowledge(&self, id: &str, by: &str) -> Result<Json, EdgeError> {
        let request = Json::obj(vec![("id", Json::s(id)), ("by", Json::s(by))]);
        let response = client::call(
            &self.identity,
            &self.addr,
            &self.purpose,
            ACKNOWLEDGE_ALERT,
            request.encode().as_bytes(),
        )
        .map_err(|e| EdgeError::new(502, &format!("Heimdall is unreachable: {e}")))?;
        alerts_response(&response)
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
    fn alert_params_and_paths_parse_and_refuse() {
        assert!(alerts_state("").unwrap());
        assert!(alerts_state("state=open").unwrap());
        assert!(!alerts_state("state=all").unwrap());
        assert!(alerts_state("other=1&state=").unwrap());
        assert_eq!(alerts_state("state=closed").unwrap_err().status, 400);
        assert_eq!(alerts_state("state=%zz").unwrap_err().status, 400);

        assert_eq!(
            ack_target("/api/v1/heimdall/alerts/A-12/ack").map(|r| r.unwrap()),
            Some("A-12".to_string())
        );
        assert_eq!(
            ack_target("/api/v1/heimdall/alerts/A%2D1/ack").map(|r| r.unwrap()),
            Some("A-1".to_string())
        );
        for bad in [
            "/api/v1/heimdall/alerts//ack",
            "/api/v1/heimdall/alerts/A 1/ack",
            "/api/v1/heimdall/alerts/A%201/ack",
            "/api/v1/heimdall/alerts/A/1/ack",
            "/api/v1/heimdall/alerts/%zz/ack",
        ] {
            assert_eq!(ack_target(bad).unwrap().unwrap_err().status, 400, "{bad}");
        }
        for not_ack in [
            "/api/v1/heimdall/alerts",
            "/api/v1/heimdall/alerts/A-1",
            "/api/v1/heimdall/alertsA-1/ack",
            "/api/v1/heimdall/picture",
        ] {
            assert!(ack_target(not_ack).is_none(), "{not_ack}");
        }
    }

    #[test]
    fn picture_pages_merge_and_refusals_map_to_status() {
        let page = |json: &str| {
            let mut bytes = vec![ACCEPTED];
            bytes.extend_from_slice(json.as_bytes());
            picture_page(&bytes).unwrap()
        };
        let mut merged = MergedPicture::default();
        let next = merged
            .add(&page(
                r#"{"layers":[{"id":"air"}],"entities":[{"id":"A"},{"id":"B"}],"next":"B"}"#,
            ))
            .unwrap();
        assert_eq!(next, Json::s("B"));
        let next = merged
            .add(&page(
                r#"{"layers":[{"id":"air"},{"id":"sea"}],"entities":[{"id":"C"}],"next":null}"#,
            ))
            .unwrap();
        assert_eq!(next, Json::Null);
        assert_eq!(
            merged.into_json(false).encode(),
            parse(r#"{"layers":[{"id":"air"}],"entities":[{"id":"A"},{"id":"B"},{"id":"C"}]}"#)
                .unwrap()
                .encode(),
            "layers from the first page; no truncated flag when whole"
        );
        let mut old = MergedPicture::default();
        assert_eq!(
            old.add(&page(r#"{"layers":[],"entities":[]}"#)).unwrap(),
            Json::Null,
            "a page with no cursor is the last"
        );
        assert_eq!(
            old.into_json(true).get("truncated"),
            Some(&Json::Bool(true))
        );
        assert!(MergedPicture::default()
            .add(&page(r#"{"layers":[]}"#))
            .is_err());
        assert!(MergedPicture::default()
            .add(&page(r#"{"layers":[],"entities":[],"next":5}"#))
            .is_err());
        assert_eq!(picture_page(b"\x01denied").unwrap_err().status, 502);
        assert_eq!(picture_page(b"\x00nope").unwrap_err().status, 502);
        assert_eq!(picture_page(&[]).unwrap_err().status, 502);
    }

    #[test]
    fn alert_responses_map_to_status() {
        let ok = alerts_response(b"\x00{\"id\":\"A-1\"}").unwrap();
        assert_eq!(ok.str_field("id"), Some("A-1"));
        assert_eq!(
            alerts_response(b"\x04there is no alert A-9")
                .unwrap_err()
                .status,
            404
        );
        assert_eq!(alerts_response(b"\x02bad").unwrap_err().status, 400);
        assert_eq!(alerts_response(b"\x01denied").unwrap_err().status, 502);
        assert_eq!(alerts_response(b"\x03failed").unwrap_err().status, 502);
        assert_eq!(alerts_response(b"\x00not json").unwrap_err().status, 502);
        assert_eq!(alerts_response(&[]).unwrap_err().status, 502);
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
