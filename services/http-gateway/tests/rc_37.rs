//! RC-37 — a console launched in production mode authenticates and renders live
//! enclosure-edge data (not client-side fixtures). Here the backend half: the
//! `/api/v1/login` endpoint exchanges credentials for a PQ-signed token, and that
//! token authorises subsequent data requests against the edge. [RC-37] [RC-29]
#![cfg(feature = "backend-fips")]

use http_gateway::http::{Request, Response};
use http_gateway::{SecuredGateway, DEMO_PASSWORD, DEMO_USERNAME};

const NOW: u64 = 1_000_000;

fn post(gw: &SecuredGateway, path: &str, body: &str, bearer: Option<&str>) -> Response {
    let req = Request {
        method: "POST".into(),
        path: path.into(),
        query: String::new(),
        body: body.into(),
        authorization: bearer.map(|t| format!("Bearer {t}")),
        origin: None,
    };
    gw.handle(&req, NOW)
}

/// Pull `"token":"..."` out of the login response JSON (hand-rolled, no serde).
fn token_of(body: &str) -> Option<String> {
    let key = "\"token\":\"";
    let start = body.find(key)? + key.len();
    let rest = &body[start..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

#[test]
fn login_issues_a_pq_token_that_authorises_data_requests() {
    let gw = SecuredGateway::dev();

    // Unauthenticated data request is rejected (401) — the console must log in first.
    let anon = post(&gw, "/api/v1/query", "{\"objectType\":\"aircraft\"}", None);
    assert_eq!(anon.status, 401);

    // Wrong password → uniform 401, no token.
    let bad = post(
        &gw,
        "/api/v1/login",
        "{\"username\":\"operator\",\"password\":\"wrong\"}",
        None,
    );
    assert_eq!(bad.status, 401);
    assert!(token_of(&bad.body).is_none());

    // Correct credentials → 200 + a PQ-signed bearer token.
    let creds = format!("{{\"username\":\"{DEMO_USERNAME}\",\"password\":\"{DEMO_PASSWORD}\"}}");
    let ok = post(&gw, "/api/v1/login", &creds, None);
    assert_eq!(ok.status, 200);
    assert!(ok.body.contains("\"subject\":\"operator\""));
    let token = token_of(&ok.body).expect("login returns a token");
    assert!(!token.is_empty());

    // The issued token authorises a data request (served, not 401/403) — the console
    // now renders enclosure-edge data rather than client-side fixtures.
    let served = post(
        &gw,
        "/api/v1/query",
        "{\"objectType\":\"aircraft\"}",
        Some(&token),
    );
    assert_eq!(
        served.status, 200,
        "authenticated query is served: {}",
        served.body
    );
}

#[test]
fn a_forged_token_is_still_rejected() {
    let gw = SecuredGateway::dev();
    // A syntactically-valid but non-issued token must not authenticate.
    let forged = post(
        &gw,
        "/api/v1/query",
        "{\"objectType\":\"aircraft\"}",
        Some("deadbeef"),
    );
    assert_eq!(forged.status, 401);
}
