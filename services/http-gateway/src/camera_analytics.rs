//! Camera analytics on request. [FR-P3-5] [FR-H-1] [NFR-AI-1]
//!
//! A signed-in user asks for one camera to be analysed (object detection and tracking on
//! its latest clip) with `POST /api/v1/heimdall/cameras/{id}/analyse`. The edge
//! authorises it as `Apply` and audits it like any action, then forwards the request to
//! the camera-analytics service (`--analytics-addr`, plain HTTP on the local host, with
//! the bearer token from `--analytics-token-file`). The service sends its result to
//! Heimdall as a signed observation and returns it here, so the browser sees it at once.
//! Nothing is analysed unless someone asks.
//!
//! A user can also ask for a camera to be watched for a few hours,
//! `POST /api/v1/heimdall/cameras/{id}/watch?hours=N` (0 stops it; `Apply` like an
//! analysis): the service then re-analyses it every few minutes, so its trend builds up.
//! `GET /api/v1/heimdall/cameras/{id}/trend` (`Read`) returns the camera's results over
//! the last day (numbers only, never images) and whether it is being watched.

use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

use service_runtime::json::{parse, Json};

use crate::enclosure::EdgeError;

/// The path prefix of the route; the camera id and `/analyse` follow.
pub const CAMERAS_PATH: &str = "/api/v1/heimdall/cameras";
/// How long one analysis may take (a clip is fetched and decoded first).
const TIMEOUT: Duration = Duration::from_secs(30);
/// The largest response accepted from the service.
const MAX_RESPONSE: usize = 256 * 1024;

/// The longest watch a user can ask for.
pub const MAX_WATCH_HOURS: i64 = 12;

/// The camera id in `/api/v1/heimdall/cameras/{id}/analyse`, if the path is that route.
pub fn analyse_target(path: &str) -> Option<Result<String, EdgeError>> {
    target(path, "/analyse")
}

/// The camera id in `/api/v1/heimdall/cameras/{id}/watch`, if the path is that route.
pub fn watch_target(path: &str) -> Option<Result<String, EdgeError>> {
    target(path, "/watch")
}

/// The camera id in `/api/v1/heimdall/cameras/{id}/trend`, if the path is that route.
pub fn trend_target(path: &str) -> Option<Result<String, EdgeError>> {
    target(path, "/trend")
}

/// The hours in a watch request's `hours` (0 to [`MAX_WATCH_HOURS`]; 0 stops the watch).
pub fn watch_hours(raw: Option<&str>) -> Result<i64, EdgeError> {
    match raw.map(|s| s.parse::<i64>()) {
        Some(Ok(h)) if (0..=MAX_WATCH_HOURS).contains(&h) => Ok(h),
        _ => Err(EdgeError::bad_request(&format!(
            "hours must be a whole number from 0 to {MAX_WATCH_HOURS}"
        ))),
    }
}

fn target(path: &str, suffix: &str) -> Option<Result<String, EdgeError>> {
    let rest = path.strip_prefix(CAMERAS_PATH)?.strip_prefix('/')?;
    let raw = rest.strip_suffix(suffix)?;
    let ok = !raw.is_empty()
        && raw.len() <= 64
        && raw
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
    Some(if ok {
        Ok(raw.to_string())
    } else {
        Err(EdgeError::bad_request(&format!(
            "'{raw}' is not a camera id"
        )))
    })
}

/// Where the analytics service is and the token it expects.
pub struct AnalyticsClient {
    addr: String,
    token: String,
}

impl AnalyticsClient {
    pub fn new(addr: &str, token: &str) -> Self {
        Self {
            addr: addr.to_string(),
            token: token.trim().to_string(),
        }
    }

    /// Ask for `camera` to be analysed on behalf of `by`: the service's JSON answer.
    /// An unknown camera is a 404, a bad id a 400; the service being unreachable or
    /// failing is a 502.
    pub fn analyse(&self, camera: &str, by: &str) -> Result<Json, EdgeError> {
        self.call(
            "/analyse",
            Json::obj(vec![("camera", Json::s(camera)), ("by", Json::s(by))]),
        )
    }

    /// Watch `camera` for `hours` on behalf of `by` (0 stops the watch).
    pub fn watch(&self, camera: &str, hours: i64, by: &str) -> Result<Json, EdgeError> {
        self.call(
            "/watch",
            Json::obj(vec![
                ("camera", Json::s(camera)),
                ("hours", Json::Num(hours as f64)),
                ("by", Json::s(by)),
            ]),
        )
    }

    /// The camera's results over the last day and its watch.
    pub fn trend(&self, camera: &str) -> Result<Json, EdgeError> {
        self.call("/trend", Json::obj(vec![("camera", Json::s(camera))]))
    }

    fn call(&self, path: &str, body: Json) -> Result<Json, EdgeError> {
        let unreachable =
            |e: String| EdgeError::new(502, &format!("camera analytics is unreachable: {e}"));
        let body = body.encode();
        let addr = self
            .addr
            .to_socket_addrs()
            .map_err(|e| unreachable(e.to_string()))?
            .next()
            .ok_or_else(|| unreachable("no address".into()))?;
        let mut stream =
            TcpStream::connect_timeout(&addr, TIMEOUT).map_err(|e| unreachable(e.to_string()))?;
        stream.set_read_timeout(Some(TIMEOUT)).ok();
        stream.set_write_timeout(Some(TIMEOUT)).ok();
        let request = format!(
            "POST {} HTTP/1.1\r\nHost: {}\r\nAuthorization: Bearer {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            path,
            self.addr,
            self.token,
            body.len(),
            body
        );
        stream
            .write_all(request.as_bytes())
            .map_err(|e| unreachable(e.to_string()))?;
        let mut raw = Vec::new();
        stream
            .take(MAX_RESPONSE as u64 + 1)
            .read_to_end(&mut raw)
            .map_err(|e| unreachable(e.to_string()))?;
        if raw.len() > MAX_RESPONSE {
            return Err(EdgeError::new(502, "camera analytics answered too much"));
        }
        response(&raw)
    }
}

/// The service's HTTP response as JSON, or the error the browser gets.
fn response(raw: &[u8]) -> Result<Json, EdgeError> {
    let text = String::from_utf8_lossy(raw);
    let (head, body) = text
        .split_once("\r\n\r\n")
        .ok_or_else(|| EdgeError::new(502, "camera analytics sent a malformed response"))?;
    let status: u16 = head
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| EdgeError::new(502, "camera analytics sent no status"))?;
    let json =
        parse(body).map_err(|_| EdgeError::new(502, "camera analytics sent malformed JSON"))?;
    let message = json
        .str_field("error")
        .unwrap_or("camera analytics failed")
        .to_string();
    match status {
        200 => Ok(json),
        400 => Err(EdgeError::bad_request(&message)),
        404 => Err(EdgeError::new(404, &message)),
        _ => Err(EdgeError::new(502, &message)),
    }
}

#[cfg(test)]
mod tests {
    use std::net::TcpListener;
    use std::thread;

    use super::*;

    #[test]
    fn the_route_names_a_camera() {
        let id = |p: &str| analyse_target(p).map(|r| r.map_err(|e| e.status));
        assert_eq!(
            id("/api/v1/heimdall/cameras/JamCams_00001.04227/analyse"),
            Some(Ok("JamCams_00001.04227".into()))
        );
        assert_eq!(id("/api/v1/heimdall/cameras/a%20b/analyse"), Some(Err(400)));
        assert_eq!(id("/api/v1/heimdall/cameras//analyse"), Some(Err(400)));
        assert_eq!(id("/api/v1/heimdall/alerts/A-1/ack"), None);
        assert_eq!(
            watch_target("/api/v1/heimdall/cameras/CAM-1/watch").map(|r| r.ok()),
            Some(Some("CAM-1".into()))
        );
        assert_eq!(
            trend_target("/api/v1/heimdall/cameras/CAM-1/trend").map(|r| r.ok()),
            Some(Some("CAM-1".into()))
        );
        assert!(analyse_target("/api/v1/heimdall/cameras/CAM-1/trend").is_none());
    }

    #[test]
    fn a_watch_is_zero_to_twelve_hours() {
        assert_eq!(watch_hours(Some("4")).ok(), Some(4));
        assert_eq!(watch_hours(Some("0")).ok(), Some(0));
        for bad in [Some("13"), Some("-1"), Some("two"), None] {
            assert_eq!(watch_hours(bad).unwrap_err().status, 400);
        }
    }

    /// A one-shot service on a free port that checks the token and answers `reply`.
    fn service(reply: &'static str) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            let mut buf = [0u8; 4096];
            let n = s.read(&mut buf).unwrap();
            let req = String::from_utf8_lossy(&buf[..n]).to_string();
            let answer = if req.contains("Authorization: Bearer tok")
                && req.contains("\"camera\":\"CAM-1\"")
            {
                reply
            } else {
                "HTTP/1.1 401 Unauthorized\r\n\r\n{\"error\":\"unauthorised\"}"
            };
            s.write_all(answer.as_bytes()).unwrap();
        });
        addr
    }

    #[test]
    fn a_request_reaches_the_service_and_its_answer_comes_back() {
        let ok = service("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\r\n{\"camera\":\"CAM-1\",\"attrs\":{\"ai_vehicles\":3}}");
        let got = AnalyticsClient::new(&ok, "tok")
            .analyse("CAM-1", "analyst")
            .unwrap();
        assert_eq!(got.str_field("camera"), Some("CAM-1"));

        let missing =
            service("HTTP/1.1 404 Not Found\r\n\r\n{\"error\":\"no camera CAM-1 with media\"}");
        assert_eq!(
            AnalyticsClient::new(&missing, "tok")
                .analyse("CAM-1", "a")
                .unwrap_err()
                .status,
            404
        );

        let wrong_token = service("HTTP/1.1 200 OK\r\n\r\n{}");
        assert_eq!(
            AnalyticsClient::new(&wrong_token, "other")
                .analyse("CAM-1", "a")
                .unwrap_err()
                .status,
            502
        );

        assert_eq!(
            AnalyticsClient::new("127.0.0.1:1", "tok")
                .analyse("CAM-1", "a")
                .unwrap_err()
                .status,
            502
        );
    }
}
