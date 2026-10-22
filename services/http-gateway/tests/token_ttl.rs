//! `--token-ttl`: a presenter's session can outlast the default hour. The gateway
//! mints login tokens for the configured lifetime and reports it as `expiresAt`;
//! the CLI accepts 60..=86400 seconds and nothing else.
#![cfg(feature = "backend-fips")]

use http_gateway::http::{Request, Response};
use http_gateway::{
    parse_token_ttl, SecuredGateway, DEFAULT_TOKEN_TTL, DEMO_PASSWORD, DEMO_USERNAME,
    MAX_TOKEN_TTL, MIN_TOKEN_TTL,
};

const NOW: u64 = 1_000_000;

fn post(gw: &SecuredGateway, path: &str, body: &str, bearer: Option<&str>, now: u64) -> Response {
    let req = Request {
        method: "POST".into(),
        path: path.into(),
        query: String::new(),
        body: body.into(),
        authorization: bearer.map(|t| format!("Bearer {t}")),
        origin: None,
    };
    gw.handle(&req, now)
}

fn field<'a>(body: &'a str, name: &str) -> Option<&'a str> {
    let key = format!("\"{name}\":\"");
    let start = body.find(&key)? + key.len();
    let rest = &body[start..];
    Some(&rest[..rest.find('"')?])
}

fn login(gw: &SecuredGateway) -> (String, u64) {
    let creds = format!("{{\"username\":\"{DEMO_USERNAME}\",\"password\":\"{DEMO_PASSWORD}\"}}");
    let ok = post(gw, "/api/v1/login", &creds, None, NOW);
    assert_eq!(ok.status, 200, "login: {}", ok.body);
    let token = field(&ok.body, "token").expect("token").to_string();
    let expires = field(&ok.body, "expiresAt")
        .expect("expiresAt")
        .parse()
        .expect("expiresAt is seconds");
    (token, expires)
}

fn query(gw: &SecuredGateway, token: &str, now: u64) -> u16 {
    post(
        gw,
        "/api/v1/query",
        "{\"objectType\":\"aircraft\"}",
        Some(token),
        now,
    )
    .status
}

#[test]
fn a_configured_ttl_sets_expiry_and_the_token_lapses_then() {
    let gw = SecuredGateway::dev().with_token_ttl(120);
    let (token, expires) = login(&gw);
    assert_eq!(expires, NOW + 120);
    assert_eq!(query(&gw, &token, NOW + 119), 200);
    assert_eq!(query(&gw, &token, NOW + 120), 401);
}

#[test]
fn the_default_ttl_stays_an_hour() {
    assert_eq!(DEFAULT_TOKEN_TTL, 3_600);
    let gw = SecuredGateway::dev();
    let (token, expires) = login(&gw);
    assert_eq!(expires, NOW + 3_600);
    assert_eq!(query(&gw, &token, NOW + 3_599), 200);
    assert_eq!(query(&gw, &token, NOW + 3_600), 401);
}

#[test]
fn the_parser_accepts_only_60_to_86400_seconds() {
    assert_eq!(parse_token_ttl("60"), Ok(60));
    assert_eq!(parse_token_ttl("43200"), Ok(43_200));
    assert_eq!(parse_token_ttl("86400"), Ok(86_400));
    for bad in ["59", "86401", "abc", "", "-60", "0", "120s", "+600", " 600"] {
        assert!(parse_token_ttl(bad).is_err(), "{bad:?} must be refused");
    }
}

#[test]
fn the_builder_clamps_out_of_range_ttls() {
    let (_, short) = login(&SecuredGateway::dev().with_token_ttl(0));
    assert_eq!(short, NOW + MIN_TOKEN_TTL);
    let (_, long) = login(&SecuredGateway::dev().with_token_ttl(u64::MAX));
    assert_eq!(long, NOW + MAX_TOKEN_TTL);
}
