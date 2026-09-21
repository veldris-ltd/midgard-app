//! The HTTP edge serves the **real** object fabric and governance spine: object types
//! from the registry, queries and search-around under per-object mandatory access
//! control, actions that genuinely mutate the store, policies and dry-runs from the
//! enforcing PDP, and an audit view that is the PQ-signed ledger itself — driven end
//! to end through a logged-in development gateway.
//! [DD-1] [NFR-SEC-2] [NFR-AI-1] [NFR-PQ-3] [AC-M-4]
#![cfg(feature = "backend-fips")]

use std::time::{SystemTime, UNIX_EPOCH};

use http_gateway::http::{Request, Response};
use http_gateway::json::{parse, Json};
use http_gateway::{SecuredGateway, DEMO_PASSWORD, DEMO_USERNAME};

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock after epoch")
        .as_secs()
}

fn call(
    gw: &SecuredGateway,
    method: &str,
    path: &str,
    query: &str,
    body: &str,
    token: Option<&str>,
) -> Response {
    gw.handle(
        &Request {
            method: method.into(),
            path: path.into(),
            query: query.into(),
            body: body.into(),
            authorization: token.map(|t| format!("Bearer {t}")),
            origin: None,
        },
        now(),
    )
}

/// A development gateway and a bearer token for its operator.
fn session() -> (SecuredGateway, String) {
    let gw = SecuredGateway::dev();
    let body = format!(r#"{{"username":"{DEMO_USERNAME}","password":"{DEMO_PASSWORD}"}}"#);
    let r = call(&gw, "POST", "/api/v1/login", "", &body, None);
    assert_eq!(r.status, 200, "login: {}", r.body);
    let token = field(&json(&r), "token").to_string();
    (gw, token)
}

fn json(r: &Response) -> Json {
    parse(&r.body).unwrap_or_else(|e| panic!("not JSON ({e}): {}", r.body))
}

fn items(j: &Json) -> &[Json] {
    match j {
        Json::Arr(v) => v,
        other => panic!("expected an array, got {}", other.encode()),
    }
}

fn field<'a>(j: &'a Json, key: &str) -> &'a str {
    j.str_field(key)
        .unwrap_or_else(|| panic!("no string field {key} in {}", j.encode()))
}

fn by_api_name<'a>(types: &'a [Json], api_name: &str) -> &'a Json {
    types
        .iter()
        .find(|t| field(t, "apiName") == api_name)
        .unwrap_or_else(|| panic!("no {api_name} object type"))
}

fn query(gw: &SecuredGateway, token: &str, object_type: &str) -> Response {
    let body = format!(r#"{{"objectType":"{object_type}","maxResults":50}}"#);
    call(gw, "POST", "/api/v1/query", "", &body, Some(token))
}

fn result_ids(r: &Response) -> Vec<String> {
    items(json(r).get("results").expect("results"))
        .iter()
        .map(|o| field(o, "id").to_string())
        .collect()
}

fn audit(gw: &SecuredGateway, token: &str) -> Json {
    let r = call(gw, "GET", "/api/v1/audit", "limit=200", "", Some(token));
    assert_eq!(r.status, 200, "{}", r.body);
    json(&r)
}

#[test]
fn object_types_come_from_the_registry_and_are_scoped_to_the_caller() {
    let (gw, token) = session();
    let r = call(&gw, "GET", "/api/v1/object-types", "", "", Some(&token));
    assert_eq!(r.status, 200, "{}", r.body);
    let body = json(&r);
    let types = items(&body);

    let aircraft = by_api_name(types, "aircraft");
    assert_eq!(field(aircraft, "name"), "Aircraft");
    assert_eq!(field(aircraft, "instanceCount"), "4");
    assert_eq!(field(aircraft, "marking"), "confidential");
    assert_eq!(field(aircraft, "compartment"), "OPS-N");
    assert!(items(aircraft.get("actions").expect("actions"))
        .iter()
        .any(|a| field(a, "name") == "Ground aircraft"
            && matches!(a.get("humanGated"), Some(Json::Bool(true)))));

    // Two sensor feeds are seeded; one is SIG-compartmented and the operator does not
    // hold SIG, so the operator's count is one — the listing is not an oracle.
    let feeds = by_api_name(types, "sensor_feed");
    assert_eq!(field(feeds, "instanceCount"), "1");
    assert!(items(feeds.get("actions").expect("actions"))
        .iter()
        .any(|a| field(a, "name") == "Recalibrate"
            && matches!(a.get("humanGated"), Some(Json::Bool(false)))));
}

#[test]
fn a_query_returns_real_objects_and_its_audit_id_is_a_real_ledger_row() {
    let (gw, token) = session();
    let r = query(&gw, &token, "aircraft");
    assert_eq!(r.status, 200, "{}", r.body);
    assert!(result_ids(&r).contains(&"aircraft/AC-2214".to_string()));

    let body = json(&r);
    let audit_id = field(&body, "auditId").to_string();
    assert!(audit_id.starts_with("AUD-"), "{audit_id}");
    let ledger = audit(&gw, &token);
    assert!(
        items(&ledger)
            .iter()
            .any(|row| field(row, "id") == audit_id && field(row, "action") == "PERMIT Read"),
        "query decision {audit_id} is not in the ledger: {}",
        ledger.encode()
    );
}

#[test]
fn mandatory_access_control_hides_objects_outside_the_callers_compartments() {
    let (gw, token) = session();
    let ids = result_ids(&query(&gw, &token, "sensor_feed"));
    assert!(
        ids.contains(&"sensor_feed/SF-TRANSPONDER-4".to_string()),
        "{ids:?}"
    );
    assert!(
        !ids.contains(&"sensor_feed/SF-NORTH-2".to_string()),
        "SIG feed leaked: {ids:?}"
    );
}

#[test]
fn an_unknown_object_type_is_404_not_an_empty_result() {
    let (gw, token) = session();
    assert_eq!(query(&gw, &token, "no_such_type").status, 404);
}

#[test]
fn a_gated_action_without_a_human_gate_token_is_refused_and_changes_nothing() {
    let (gw, token) = session();
    let r = call(
        &gw,
        "POST",
        "/api/v1/actions",
        "",
        r#"{"actionType":"Ground aircraft","params":{"target":"aircraft/*"}}"#,
        Some(&token),
    );
    assert_eq!(r.status, 403, "{}", r.body);
    assert!(r.body.contains("Human gate required"), "{}", r.body);
    assert!(!query(&gw, &token, "aircraft").body.contains("grounded"));
}

#[test]
fn an_authorised_gated_action_really_changes_the_fabric() {
    let (gw, token) = session();
    let r = call(
        &gw,
        "POST",
        "/api/v1/actions",
        "",
        r#"{"actionType":"Ground aircraft","params":{"target":"aircraft/AC-2214"},"humanGateToken":"HG-test"}"#,
        Some(&token),
    );
    assert_eq!(r.status, 200, "{}", r.body);
    let body = json(&r);
    assert!(matches!(body.get("applied"), Some(Json::Bool(true))));
    let action_audit = field(&body, "auditId").to_string();

    let after = json(&query(&gw, &token, "aircraft"));
    let results = items(after.get("results").expect("results"));
    let status_of = |id: &str| {
        let obj = results
            .iter()
            .find(|o| field(o, "id") == id)
            .unwrap_or_else(|| panic!("{id} missing"));
        field(obj.get("propertyValues").expect("properties"), "status").to_string()
    };
    assert_eq!(status_of("aircraft/AC-2214"), "grounded");
    assert_eq!(
        status_of("aircraft/AC-2261"),
        "active",
        "only the named target changes"
    );

    let ledger = audit(&gw, &token);
    assert!(
        items(&ledger)
            .iter()
            .any(|row| field(row, "id") == action_audit
                && field(row, "action") == "PERMIT groundAircraft"),
        "action {action_audit} is not in the ledger: {}",
        ledger.encode()
    );
}

#[test]
fn an_ungated_action_needs_no_token_and_applies_only_to_readable_objects() {
    let (gw, token) = session();
    let r = call(
        &gw,
        "POST",
        "/api/v1/actions",
        "",
        r#"{"actionType":"Recalibrate","params":{"target":"sensor_feed/*"}}"#,
        Some(&token),
    );
    assert_eq!(r.status, 200, "{}", r.body);
    let after = json(&query(&gw, &token, "sensor_feed"));
    let feeds = items(after.get("results").expect("results"));
    assert_eq!(feeds.len(), 1);
    assert_eq!(
        field(
            feeds[0].get("propertyValues").expect("properties"),
            "status"
        ),
        "calibrating"
    );
}

#[test]
fn an_action_on_an_unreadable_target_is_indistinguishable_from_a_missing_one() {
    let (gw, token) = session();
    let act = |target: &str| {
        let body = format!(r#"{{"actionType":"Recalibrate","params":{{"target":"{target}"}}}}"#);
        call(&gw, "POST", "/api/v1/actions", "", &body, Some(&token))
    };
    let hidden = act("sensor_feed/SF-NORTH-2");
    let missing = act("sensor_feed/SF-DOES-NOT-EXIST");
    assert_eq!(hidden.status, 404, "{}", hidden.body);
    assert_eq!(missing.status, 404, "{}", missing.body);
    assert_eq!(
        hidden.body.replace("SF-NORTH-2", "X"),
        missing.body.replace("SF-DOES-NOT-EXIST", "X"),
        "the edge must not reveal which objects exist above the caller's clearance"
    );
}

#[test]
fn search_around_walks_real_links_in_both_directions_under_access_control() {
    let (gw, token) = session();
    let walk = |seed: &str| {
        let body = format!(r#"{{"seedId":"{seed}","hops":2}}"#);
        let r = call(
            &gw,
            "POST",
            "/api/v1/search-around",
            "",
            &body,
            Some(&token),
        );
        assert_eq!(r.status, 200, "{}", r.body);
        json(&r)
    };

    let graph = walk("aircraft/AC-2214");
    let hop_of = |id: &str| {
        items(graph.get("nodes").expect("nodes"))
            .iter()
            .find(|n| field(n, "id") == id)
            .and_then(|n| n.get("hop"))
            .and_then(Json::as_i64)
    };
    assert_eq!(hop_of("aircraft/AC-2214"), Some(0));
    assert_eq!(hop_of("route/RT-N-S"), Some(1), "outgoing: operates");
    assert_eq!(hop_of("crew_member/CR-101"), Some(1), "outgoing: assigned");
    assert_eq!(
        hop_of("maintenance_event/ME-88121"),
        Some(1),
        "incoming: on"
    );
    assert_eq!(
        hop_of("sensor_feed/SF-TRANSPONDER-4"),
        Some(1),
        "incoming: observes"
    );
    assert_eq!(
        hop_of("aircraft/AC-2261"),
        Some(2),
        "via a shared route or base"
    );
    assert!(items(graph.get("edges").expect("edges"))
        .iter()
        .any(|e| field(e, "label") == "operates → Route"));

    // AC-2261 is observed by the SIG feed the operator cannot read.
    let other = walk("aircraft/AC-2261");
    assert!(!other.encode().contains("SF-NORTH-2"), "{}", other.encode());
}

#[test]
fn every_audit_row_is_verified_against_its_ml_dsa_signature() {
    let (gw, token) = session();
    assert_eq!(query(&gw, &token, "aircraft").status, 200);
    let ledger = audit(&gw, &token);
    let rows = items(&ledger);
    assert!(
        rows.len() >= 3,
        "admission, query and audit-read decisions should be present"
    );
    for row in rows {
        let sig = field(row, "pqSignature");
        assert!(
            sig.starts_with("ML-DSA-65") && sig.ends_with('✓'),
            "unverified row: {}",
            row.encode()
        );
    }
}

#[test]
fn policies_are_the_rule_set_the_pdp_enforces() {
    let (gw, token) = session();
    let r = call(&gw, "GET", "/api/v1/policies", "", "", Some(&token));
    assert_eq!(r.status, 200, "{}", r.body);
    let body = json(&r);
    let ids: Vec<&str> = items(&body).iter().map(|p| field(p, "id")).collect();
    assert!(ids.contains(&"operator-read"), "{ids:?}");
    assert!(ids.contains(&"operator-ground_aircraft"), "{ids:?}");
}

#[test]
fn the_policy_dry_run_is_a_real_pdp_decision_under_the_sessions_purpose() {
    let (gw, token) = session();
    let simulate = |purpose: &str, marking: &str| {
        let body = format!(r#"{{"purpose":"{purpose}","marking":"{marking}"}}"#);
        json(&call(
            &gw,
            "POST",
            "/api/v1/policy/simulate",
            "",
            &body,
            Some(&token),
        ))
    };

    let grant = simulate("operations", "confidential");
    assert_eq!(field(&grant, "decision"), "grant", "{}", grant.encode());
    assert_eq!(field(&grant, "matchedRule"), "operator-read");
    assert!(field(&grant, "auditId").starts_with("AUD-"));

    // The session's token authorises "operations" only, so a request under any other
    // purpose meets the PDP's purpose gate.
    let deny = simulate("export", "confidential");
    assert_eq!(field(&deny, "decision"), "deny", "{}", deny.encode());
    assert!(matches!(deny.get("matchedRule"), Some(Json::Null)));
    assert!(
        field(&deny, "reason").contains("\"export\""),
        "{}",
        deny.encode()
    );
}

#[test]
fn edge_admission_and_fabric_decisions_share_one_ledger() {
    let (gw, token) = session();
    let before = gw.audit_len();
    assert_eq!(query(&gw, &token, "aircraft").status, 200);
    assert_eq!(
        gw.audit_len(),
        before + 2,
        "one admission decision at the edge and one read decision in the fabric, same chain"
    );
}
