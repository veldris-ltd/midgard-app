//! RC-24: the HTTP/JSON edge is a *secured* boundary. It verifies the PQ-signed
//! IdP bearer token on every request, authorises + audits each through the real
//! policy PDP / governance spine, and locks CORS to a configured origin allowlist.
//!
//! - An **unauthenticated** request is rejected (401) and never reaches the data.
//! - An **above-clearance** query is denied (403) AND audited by governance.
//! - An **in-clearance** authenticated request is served, over an origin-locked
//!   channel (the allowlisted origin is echoed; a foreign origin gets no grant).
//!
//! This is the RC-24 acceptance gate. [RC-24] [NFR-SEC-1]

use std::collections::BTreeSet;

use governance::Governance;
use http_gateway::http::Request;
use http_gateway::SecuredGateway;
use iam::{Idp, UserRecord};
use policy::{
    Action, ClassificationLevel, Clearance, DefaultPdp, Effect, PolicySet, Purpose, Role, Rule,
    RuleId, Selector,
};
use pqc_agility::backend::fips::provider as fips_provider;
use pqc_agility::OsRng;

const NOW: u64 = 1_000_000;
const TTL: u64 = 3_600;

/// Lowercase-hex encode (the edge expects the bearer token hex-encoded).
fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

fn role(name: &str) -> Role {
    Role(name.to_string())
}
fn purpose(name: &str) -> Purpose {
    Purpose(name.to_string())
}

/// A permit rule: the Analyst role may Read anything, for the Investigation purpose.
/// (Mandatory clearance dominance is still enforced first by the PDP.)
fn analyst_may_read() -> PolicySet {
    PolicySet::new(vec![Rule {
        id: RuleId("analyst-read".to_string()),
        role: role("Analyst"),
        action: Action("Read".to_string()),
        purpose: Some(purpose("Investigation")),
        selector: Selector::Any,
        effect: Effect::Permit,
    }])
    .expect("policy set")
}

/// Build an IdP that has registered one analyst with the given clearance, and mint
/// a token for them; return (gateway, hex-encoded-token). The gateway classifies
/// object-type `case-secret` as SECRET.
fn fixture(analyst_level: ClassificationLevel) -> (SecuredGateway, String) {
    let mut rng = OsRng;
    let mut idp = Idp::generate("test-idp", &mut rng).unwrap();
    idp.store_mut().upsert(
        "analyst-1",
        UserRecord {
            roles: [role("Analyst")].into_iter().collect(),
            clearance: Clearance {
                level: analyst_level,
                compartments: BTreeSet::new(),
            },
            purposes: [purpose("Investigation")].into_iter().collect(),
        },
    );
    let token = idp
        .issue_token("analyst-1", purpose("Investigation"), NOW, TTL)
        .expect("issue token");

    let governance = Governance::new(DefaultPdp::new(analyst_may_read()), &mut rng).unwrap();
    let gw = SecuredGateway::new(
        fips_provider(),
        idp.verifying_key().clone(),
        governance,
        vec!["http://localhost:5173".to_string()],
    )
    .with_marking("case-secret", ClassificationLevel::SECRET)
    .with_marking("case-open", ClassificationLevel::UNCLASSIFIED)
    // The gateway serves the real fabric, which fails closed on an unknown object
    // type — so the types these requests name must exist.
    .with_object_type("case-secret", "Case (secret)")
    .with_object_type("case-open", "Case (open)");
    (gw, hex(&token))
}

fn query(object_type: &str, token: Option<&str>, origin: Option<&str>) -> Request {
    Request {
        method: "POST".to_string(),
        path: "/api/v1/query".to_string(),
        query: String::new(),
        body: format!("{{\"objectType\":\"{object_type}\"}}"),
        authorization: token.map(|t| format!("Bearer {t}")),
        origin: origin.map(str::to_string),
    }
}

#[test]
fn unauthenticated_request_is_rejected_401() {
    let (gw, _token) = fixture(ClassificationLevel::SECRET);
    // No Authorization header at all.
    let resp = gw.handle(&query("case-open", None, None), NOW);
    assert_eq!(resp.status, 401, "no token must be rejected");
    // And a garbage token is likewise rejected, not served.
    let resp = gw.handle(&query("case-open", Some("deadbeef"), None), NOW);
    assert_eq!(resp.status, 401, "forged/garbage token must be rejected");
}

#[test]
fn above_clearance_query_is_denied_403_and_audited() {
    // A CONFIDENTIAL analyst querying a SECRET-marked resource: read-up, must deny.
    let (gw, token) = fixture(ClassificationLevel::CONFIDENTIAL);
    let before = gw.audit_len();
    let resp = gw.handle(&query("case-secret", Some(&token), None), NOW);
    assert_eq!(resp.status, 403, "read-up must be denied by the PDP");
    assert!(
        gw.audit_len() > before,
        "the denial must be audited by governance"
    );
    assert!(
        resp.body.contains("auditSeq"),
        "denial carries an audit seq"
    );
}

#[test]
fn in_clearance_request_is_served_over_origin_locked_channel() {
    // A SECRET analyst querying a SECRET resource, from the allowlisted origin.
    let (gw, token) = fixture(ClassificationLevel::SECRET);
    let before = gw.audit_len();
    let resp = gw.handle(
        &query("case-secret", Some(&token), Some("http://localhost:5173")),
        NOW,
    );
    assert_eq!(
        resp.status, 200,
        "in-clearance, permitted read must be served"
    );
    // Permit is audited too.
    assert!(gw.audit_len() > before, "the permit is audited");
    // Origin-locked CORS: the allowlisted origin is echoed exactly (never `*`).
    assert_eq!(
        resp.allow_origin.as_deref(),
        Some("http://localhost:5173"),
        "allowlisted origin echoed"
    );

    // A foreign origin gets NO CORS grant, even though the request itself succeeds.
    let resp = gw.handle(
        &query("case-secret", Some(&token), Some("https://evil.example")),
        NOW,
    );
    assert_eq!(resp.status, 200);
    assert_eq!(
        resp.allow_origin, None,
        "a non-allowlisted origin must not receive a CORS grant"
    );
}

#[test]
fn public_health_needs_no_token() {
    let (gw, _token) = fixture(ClassificationLevel::SECRET);
    let health = Request {
        method: "GET".to_string(),
        path: "/api/v1/health".to_string(),
        query: String::new(),
        body: String::new(),
        authorization: None,
        origin: None,
    };
    let resp = gw.handle(&health, NOW);
    assert_eq!(
        resp.status, 200,
        "liveness is the only unauthenticated route"
    );
}
