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
    let (gw, token, _idp) = fixture_with_idp(analyst_level);
    (gw, token)
}

/// An IdP with the fixture's analyst registered, under `issuer_id`.
fn idp_with_analyst(issuer_id: &str, analyst_level: ClassificationLevel) -> Idp {
    let mut idp = Idp::generate(issuer_id, &mut OsRng).unwrap();
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
    idp
}

/// As [`fixture`], also handing back the IdP the gateway trusts, so a test can mint
/// further tokens from it.
fn fixture_with_idp(analyst_level: ClassificationLevel) -> (SecuredGateway, String, Idp) {
    let mut rng = OsRng;
    let idp = idp_with_analyst("test-idp", analyst_level);
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
    (gw, hex(&token), idp)
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

// ---- negative paths: token lifecycle and malformed headers -----------------------------
//
// Every refusal below is a 401 that names its reason, carries no claims, and writes no
// audit row: an unauthenticated request never reaches the object-specific audit. (WP-26
// may add an authentication-failure record; these assertions then become plus one.)
// [AC-42.1] [AC-42.2] [VLD-ANB-MIDGARD-0092]

/// A request with this exact `Authorization` header value (or none).
fn query_with_authorization(authorization: Option<String>) -> Request {
    Request {
        authorization,
        ..query("case-open", None, None)
    }
}

/// `req` at `now` is refused 401 with `reason` in the body, no claims, and no audit row.
fn assert_refused(gw: &SecuredGateway, req: &Request, now: u64, reason: &str) {
    let before = gw.audit_len();
    let resp = gw.handle(req, now);
    assert_eq!(
        resp.status, 401,
        "{:?} must be refused: {}",
        req.authorization, resp.body
    );
    assert!(
        resp.body.contains(reason),
        "the refusal names {reason:?}: {}",
        resp.body
    );
    assert!(
        !resp.body.contains("subject") && !resp.body.contains("analyst-1"),
        "a refusal carries no claims: {}",
        resp.body
    );
    assert_eq!(
        gw.audit_len(),
        before,
        "a refused token writes no audit row"
    );
}

#[test]
fn an_expired_token_is_rejected_401() {
    let (gw, token) = fixture(ClassificationLevel::SECRET);
    // Expiry is exclusive: the last second of the token's life is served...
    let resp = gw.handle(&query("case-open", Some(&token), None), NOW + TTL - 1);
    assert_eq!(resp.status, 200, "a token is valid until its expiry");
    // ...and the second it expires is not.
    assert_refused(
        &gw,
        &query("case-open", Some(&token), None),
        NOW + TTL,
        "Expired",
    );
}

#[test]
fn a_not_yet_valid_token_is_rejected_401() {
    let (gw, _token, idp) = fixture_with_idp(ClassificationLevel::SECRET);
    let early = idp
        .issue_token("analyst-1", purpose("Investigation"), NOW + 100, TTL)
        .expect("issue token");
    assert_refused(
        &gw,
        &query("case-open", Some(&hex(&early)), None),
        NOW,
        "NotYetValid",
    );
}

#[test]
fn a_token_from_another_issuer_is_rejected_401() {
    // Tokens carry no audience field: the gateway is bound to one issuer's verifying
    // key, and that binding is the audience control. A token another IdP signed for
    // the same user, purpose and time does not verify against it.
    let (gw, _token) = fixture(ClassificationLevel::SECRET);
    let other = idp_with_analyst("other-idp", ClassificationLevel::SECRET);
    let foreign = other
        .issue_token("analyst-1", purpose("Investigation"), NOW, TTL)
        .expect("issue token");
    assert_refused(
        &gw,
        &query("case-open", Some(&hex(&foreign)), None),
        NOW,
        "BadSignature",
    );
}

#[test]
fn a_non_hex_bearer_is_rejected_401() {
    let (gw, _token) = fixture(ClassificationLevel::SECRET);
    assert_refused(
        &gw,
        &query("case-open", Some("zz-not-hex"), None),
        NOW,
        "malformed bearer token",
    );
}

#[test]
fn a_missing_scheme_is_rejected_401() {
    let (gw, token) = fixture(ClassificationLevel::SECRET);
    // A valid token without the `Bearer ` scheme is not read as one.
    assert_refused(
        &gw,
        &query_with_authorization(Some(token)),
        NOW,
        "missing bearer token",
    );
    // Nor is another scheme.
    assert_refused(
        &gw,
        &query_with_authorization(Some("Basic dXNlcjpwdw==".to_string())),
        NOW,
        "missing bearer token",
    );
}

#[test]
fn a_token_with_trailing_bytes_is_rejected_401() {
    let (gw, token) = fixture(ClassificationLevel::SECRET);
    assert_refused(
        &gw,
        &query("case-open", Some(&format!("{token}00")), None),
        NOW,
        "Malformed",
    );
}
