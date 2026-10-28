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
use http_gateway::{
    SecuredGateway, DEMO_APPROVER_PASSWORD, DEMO_APPROVER_USERNAME, DEMO_PASSWORD, DEMO_USERNAME,
};

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
    call_at(gw, method, path, query, body, token, now())
}

fn call_at(
    gw: &SecuredGateway,
    method: &str,
    path: &str,
    query: &str,
    body: &str,
    token: Option<&str>,
    at: u64,
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
        at,
    )
}

fn login(gw: &SecuredGateway, username: &str, password: &str) -> String {
    let body = format!(r#"{{"username":"{username}","password":"{password}"}}"#);
    let r = call(gw, "POST", "/api/v1/login", "", &body, None);
    assert_eq!(r.status, 200, "login: {}", r.body);
    field(&json(&r), "token").to_string()
}

/// A development gateway and a bearer token for its operator.
fn session() -> (SecuredGateway, String) {
    let gw = SecuredGateway::dev();
    let token = login(&gw, DEMO_USERNAME, DEMO_PASSWORD);
    (gw, token)
}

/// A bearer token for the development gateway's approver.
fn approver(gw: &SecuredGateway) -> String {
    login(gw, DEMO_APPROVER_USERNAME, DEMO_APPROVER_PASSWORD)
}

/// Ask for a human gate on `action` against `target`; the pending approval's id.
fn request_approval(gw: &SecuredGateway, token: &str, action: &str, target: &str) -> String {
    let body = format!(r#"{{"actionType":"{action}","target":"{target}"}}"#);
    let r = call(gw, "POST", "/api/v1/approvals", "", &body, Some(token));
    assert_eq!(r.status, 200, "{}", r.body);
    let approval = json(&r);
    assert_eq!(field(&approval, "status"), "pending");
    field(&approval, "id").to_string()
}

fn decide(gw: &SecuredGateway, token: &str, id: &str, verb: &str) -> Response {
    let path = format!("/api/v1/approvals/{id}/{verb}");
    call(gw, "POST", &path, "", "", Some(token))
}

/// Run `action` on `target` with `gate` as its human-gate token.
fn act(gw: &SecuredGateway, token: &str, action: &str, target: &str, gate: &str) -> Response {
    let body = format!(
        r#"{{"actionType":"{action}","params":{{"target":"{target}"}},"humanGateToken":"{gate}"}}"#
    );
    call(gw, "POST", "/api/v1/actions", "", &body, Some(token))
}

/// A fresh approval of `action` on `target`, requested by the operator and approved by
/// the approver.
fn approved(gw: &SecuredGateway, token: &str, action: &str, target: &str) -> String {
    let id = request_approval(gw, token, action, target);
    let r = decide(gw, &approver(gw), &id, "approve");
    assert_eq!(r.status, 200, "{}", r.body);
    assert_eq!(field(&json(&r), "status"), "approved");
    id
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
        r#"{"actionType":"Ground aircraft","params":{"target":"aircraft/AC-2214"}}"#,
        Some(&token),
    );
    assert_eq!(r.status, 403, "{}", r.body);
    assert!(r.body.contains("Human gate required"), "{}", r.body);
    assert!(!query(&gw, &token, "aircraft").body.contains("grounded"));
}

#[test]
fn a_made_up_human_gate_token_is_refused_and_changes_nothing() {
    let (gw, token) = session();
    for gate in ["x", " ", "HG-test", "APR-1"] {
        let r = act(&gw, &token, "Ground aircraft", "aircraft/AC-2214", gate);
        assert_eq!(r.status, 403, "token {gate:?}: {}", r.body);
    }
    // A pending approval is not yet a gate either.
    let pending = request_approval(&gw, &token, "Ground aircraft", "aircraft/AC-2214");
    let r = act(&gw, &token, "Ground aircraft", "aircraft/AC-2214", &pending);
    assert_eq!(r.status, 403, "{}", r.body);
    assert!(!query(&gw, &token, "aircraft").body.contains("grounded"));
}

#[test]
fn an_action_names_one_object_and_a_wildcard_is_refused() {
    let (gw, token) = session();
    for params in [r#"{"target":"sensor_feed/*"}"#, r#"{}"#, r#"{"target":""}"#] {
        let body = format!(r#"{{"actionType":"Recalibrate","params":{params}}}"#);
        let r = call(&gw, "POST", "/api/v1/actions", "", &body, Some(&token));
        assert_eq!(r.status, 400, "{params}: {}", r.body);
    }
    let body = r#"{"actionType":"Ground aircraft","target":"aircraft/*"}"#;
    let r = call(&gw, "POST", "/api/v1/approvals", "", body, Some(&token));
    assert_eq!(r.status, 400, "{}", r.body);
    assert!(!query(&gw, &token, "sensor_feed")
        .body
        .contains("calibrating"));
}

#[test]
fn an_approval_is_bound_to_its_requester_action_and_object_and_redeems_once() {
    let (gw, token) = session();
    let id = approved(&gw, &token, "Ground aircraft", "aircraft/AC-2214");

    // Another object, another action, or another user: refused.
    let other_object = act(&gw, &token, "Ground aircraft", "aircraft/AC-2261", &id);
    assert_eq!(other_object.status, 403, "{}", other_object.body);
    let other_action = act(&gw, &token, "Schedule maintenance", "aircraft/AC-2214", &id);
    assert_eq!(other_action.status, 403, "{}", other_action.body);
    let by_approver = act(
        &gw,
        &approver(&gw),
        "Ground aircraft",
        "aircraft/AC-2214",
        &id,
    );
    assert_eq!(by_approver.status, 403, "{}", by_approver.body);

    let first = act(&gw, &token, "Ground aircraft", "aircraft/AC-2214", &id);
    assert_eq!(first.status, 200, "{}", first.body);
    let again = act(&gw, &token, "Ground aircraft", "aircraft/AC-2214", &id);
    assert_eq!(again.status, 403, "a gate is used once: {}", again.body);

    let listed = json(&call(&gw, "GET", "/api/v1/approvals", "", "", Some(&token)));
    let row = items(&listed)
        .iter()
        .find(|a| field(a, "id") == id)
        .expect("the approval is listed");
    assert_eq!(field(row, "status"), "used");
    assert_eq!(field(row, "requester"), DEMO_USERNAME);
    assert_eq!(field(row, "approver"), DEMO_APPROVER_USERNAME);
    assert!(matches!(row.get("mine"), Some(Json::Bool(true))));
}

#[test]
fn only_someone_else_holding_approve_can_decide_and_only_once() {
    let (gw, token) = session();
    let id = request_approval(&gw, &token, "Ground aircraft", "aircraft/AC-2214");

    // The operator holds no Approve rule: the edge refuses, and audits it.
    let own = decide(&gw, &token, &id, "approve");
    assert_eq!(own.status, 403, "{}", own.body);

    let approver = approver(&gw);
    let rejected = decide(&gw, &approver, &id, "reject");
    assert_eq!(rejected.status, 200, "{}", rejected.body);
    assert_eq!(field(&json(&rejected), "status"), "rejected");
    let twice = decide(&gw, &approver, &id, "approve");
    assert_eq!(twice.status, 409, "{}", twice.body);

    let r = act(&gw, &token, "Ground aircraft", "aircraft/AC-2214", &id);
    assert_eq!(r.status, 403, "a rejected approval is no gate: {}", r.body);
    assert_eq!(decide(&gw, &approver, "APR-999", "approve").status, 404);
}

#[test]
fn an_approval_expires() {
    let (gw, token) = session();
    let id = approved(&gw, &token, "Ground aircraft", "aircraft/AC-2214");
    let body = format!(
        r#"{{"actionType":"Ground aircraft","params":{{"target":"aircraft/AC-2214"}},"humanGateToken":"{id}"}}"#
    );
    // Past the approval's 15 minutes, inside the session token's hour.
    let late = call_at(
        &gw,
        "POST",
        "/api/v1/actions",
        "",
        &body,
        Some(&token),
        now() + 1_000,
    );
    assert_eq!(late.status, 403, "{}", late.body);
}

#[test]
fn an_authorised_gated_action_really_changes_the_fabric() {
    let (gw, token) = session();
    let id = approved(&gw, &token, "Ground aircraft", "aircraft/AC-2214");
    let r = act(&gw, &token, "Ground aircraft", "aircraft/AC-2214", &id);
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
fn an_ungated_action_needs_no_token_and_changes_only_its_target() {
    let (gw, token) = session();
    let r = call(
        &gw,
        "POST",
        "/api/v1/actions",
        "",
        r#"{"actionType":"Recalibrate","params":{"target":"sensor_feed/SF-TRANSPONDER-4"}}"#,
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
