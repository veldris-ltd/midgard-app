//! The HTTP edge serves the **real** object fabric and governance spine: object types
//! from the registry, queries and search-around under per-object mandatory access
//! control, actions that genuinely mutate the store, policies and dry-runs from the
//! enforcing PDP, and an audit view that is the PQ-signed ledger itself — driven end
//! to end through a logged-in development gateway.
//! The ledger view itself is scoped to the caller. [VLD-ANB-MIDGARD-0031]
//! [DD-1] [NFR-SEC-2] [NFR-AI-1] [NFR-PQ-3] [AC-M-4] [AC-20.1] [AC-20.2] [AC-20.3]
#![cfg(feature = "backend-fips")]

use std::collections::BTreeSet;
use std::time::{SystemTime, UNIX_EPOCH};

use governance::Governance;
use http_gateway::enclosure::Enclosure;
use http_gateway::http::{Request, Response};
use http_gateway::json::{parse, Json};
use http_gateway::{
    demo_approver, demo_operator, dev_login_policy, SecuredGateway, ACTION_READ_AUDIT,
    DEMO_APPROVER_PASSWORD, DEMO_APPROVER_USERNAME, DEMO_PASSWORD, DEMO_USERNAME,
};
use iam::{Idp, UserRecord};
use policy::{Clearance, DefaultPdp, PolicySet, Purpose, Rule};
use pqc_agility::backend::fips::provider as fips_provider;
use pqc_agility::OsRng;

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
fn search_around_nodes_carry_their_own_properties() {
    let (gw, token) = session();
    let body = r#"{"seedId":"aircraft/AC-2214","hops":1}"#;
    let r = call(&gw, "POST", "/api/v1/search-around", "", body, Some(&token));
    assert_eq!(r.status, 200, "{}", r.body);
    let graph = json(&r);
    let seed = items(graph.get("nodes").expect("nodes"))
        .iter()
        .find(|n| field(n, "id") == "aircraft/AC-2214")
        .expect("the seed is a node");
    let props = seed.get("propertyValues").expect("propertyValues");

    // The same values a query returns for that object, not placeholders.
    let queried = json(&query(&gw, &token, "aircraft"));
    let row = items(queried.get("results").expect("results"))
        .iter()
        .find(|o| field(o, "id") == "aircraft/AC-2214")
        .expect("AC-2214 is readable");
    assert_eq!(
        props.encode(),
        row.get("propertyValues").expect("propertyValues").encode()
    );
    assert_eq!(field(props, "status"), "active");
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

/// The search-around response, node for node and edge for edge, as the code produced
/// it before WP-43 split `search_around` into seed resolution, link collection and a
/// clearance filter. Order is pinned, not only membership: nodes in discovery order,
/// edges in the order their links were first met. [VLD-ANB-MIDGARD-0094] [AC-43.1]
/// [DD-1] [NFR-SEC-2]
#[test]
fn search_around_output_is_stable_across_the_refactor() {
    let (gw, token) = session();
    let walk = |seed: &str, hops: i64, limit: Option<i64>| {
        let body = match limit {
            Some(limit) => format!(r#"{{"seedId":"{seed}","hops":{hops},"limit":{limit}}}"#),
            None => format!(r#"{{"seedId":"{seed}","hops":{hops}}}"#),
        };
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
    let node_ids = |g: &Json| -> Vec<String> {
        items(g.get("nodes").expect("nodes"))
            .iter()
            .map(|n| field(n, "id").to_string())
            .collect()
    };
    let edges = |g: &Json| -> Vec<(String, String, String)> {
        items(g.get("edges").expect("edges"))
            .iter()
            .map(|e| {
                (
                    field(e, "from").to_string(),
                    field(e, "to").to_string(),
                    field(e, "label").to_string(),
                )
            })
            .collect()
    };
    let shape = |g: &Json| {
        (
            matches!(g.get("truncated"), Some(Json::Bool(true))),
            g.get("nodeLimit").and_then(Json::as_i64),
            g.get("seedObjects").and_then(Json::as_i64),
        )
    };

    let one = walk("aircraft/AC-2214", 2, None);
    assert_eq!(
        node_ids(&one),
        [
            "aircraft/AC-2214",
            "facility/FAC-NORTH",
            "crew_member/CR-101",
            "crew_member/CR-102",
            "route/RT-N-S",
            "maintenance_event/ME-88121",
            "sensor_feed/SF-TRANSPONDER-4",
            "aircraft/AC-2261",
        ]
    );
    let edge =
        |from: &str, to: &str, label: &str| (from.to_string(), to.to_string(), label.to_string());
    assert_eq!(
        edges(&one),
        [
            edge(
                "aircraft/AC-2214",
                "facility/FAC-NORTH",
                "based at → Facility"
            ),
            edge(
                "aircraft/AC-2214",
                "crew_member/CR-101",
                "assigned → Crew Member"
            ),
            edge(
                "aircraft/AC-2214",
                "crew_member/CR-102",
                "assigned → Crew Member"
            ),
            edge("aircraft/AC-2214", "route/RT-N-S", "operates → Route"),
            edge(
                "maintenance_event/ME-88121",
                "aircraft/AC-2214",
                "on → Aircraft"
            ),
            edge(
                "sensor_feed/SF-TRANSPONDER-4",
                "aircraft/AC-2214",
                "observes → Aircraft"
            ),
            edge(
                "aircraft/AC-2261",
                "facility/FAC-NORTH",
                "based at → Facility"
            ),
            edge("aircraft/AC-2261", "route/RT-N-S", "operates → Route"),
        ]
    );
    assert_eq!(shape(&one), (false, Some(60), Some(1)));

    // Four readable aircraft, three admitted in id order; every neighbour would be a
    // fourth node, so it and its edge are cut and the walk says so.
    let all = walk("aircraft/*", 1, Some(3));
    assert_eq!(
        node_ids(&all),
        ["aircraft/AC-2198", "aircraft/AC-2214", "aircraft/AC-2261"]
    );
    assert!(edges(&all).is_empty(), "{:?}", edges(&all));
    assert_eq!(shape(&all), (true, Some(3), Some(4)));
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
fn whoami_is_the_signed_in_user_from_their_token() {
    let (gw, token) = session();
    let strings = |j: &Json, key: &str| -> Vec<String> {
        items(j.get(key).expect(key))
            .iter()
            .map(|v| v.as_str().expect("a string").to_string())
            .collect()
    };

    let r = call(&gw, "GET", "/api/v1/whoami", "", "", Some(&token));
    assert_eq!(r.status, 200, "{}", r.body);
    let me = json(&r);
    assert_eq!(field(&me, "subject"), DEMO_USERNAME);
    assert_eq!(strings(&me, "roles"), ["operator"]);
    assert_eq!(field(&me, "clearance"), "confidential");
    assert_eq!(strings(&me, "compartments"), ["OPS-N"]);
    assert_eq!(field(&me, "purpose"), "operations");
    let expires = me
        .get("expiresAt")
        .and_then(Json::as_i64)
        .expect("expiresAt");
    assert!(expires > i64::try_from(now()).unwrap());

    let them = json(&call(
        &gw,
        "GET",
        "/api/v1/whoami",
        "",
        "",
        Some(&approver(&gw)),
    ));
    assert_eq!(field(&them, "subject"), DEMO_APPROVER_USERNAME);
    assert_eq!(strings(&them, "roles"), ["approver"]);

    assert_eq!(call(&gw, "GET", "/api/v1/whoami", "", "", None).status, 401);
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

/// The value of response header `name`, if the edge sent it.
fn header<'a>(r: &'a Response, name: &str) -> Option<&'a str> {
    r.headers
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
}

/// The audit view as `token` sees it, with the response it came in.
fn audit_view(gw: &SecuredGateway, token: &str) -> (Response, Vec<Json>) {
    let r = call(gw, "GET", "/api/v1/audit", "limit=500", "", Some(token));
    assert_eq!(r.status, 200, "{}", r.body);
    let rows = items(&json(&r)).to_vec();
    (r, rows)
}

/// A search-around from a compartmented seed writes the seed's object id to the
/// ledger. A principal cleared to the same level but without the OPS-N compartment
/// does not see that row, and is told the view was filtered; the operator, who holds
/// OPS-N, still sees it. [VLD-ANB-MIDGARD-0031] [AC-20.1] [NFR-SEC-2]
#[test]
fn the_audit_view_hides_rows_about_objects_the_caller_cannot_read() {
    let (mut gw, token) = session();
    let uncompartmented = UserRecord {
        clearance: Clearance {
            level: demo_approver().clearance.level,
            compartments: BTreeSet::new(),
        },
        ..demo_approver()
    };
    gw.provision_login("analyst", "analyst-password", uncompartmented)
        .expect("the analyst provisions in memory");

    let seed = "aircraft/AC-2214";
    let body = format!(r#"{{"seedId":"{seed}","hops":1}}"#);
    let r = call(
        &gw,
        "POST",
        "/api/v1/search-around",
        "",
        &body,
        Some(&token),
    );
    assert_eq!(r.status, 200, "{}", r.body);

    let (_, operator_rows) = audit_view(&gw, &token);
    assert!(
        operator_rows
            .iter()
            .any(|row| field(row, "resource") == format!("object:{seed}")),
        "the cleared operator should see the search-around row"
    );

    let analyst = login(&gw, "analyst", "analyst-password");
    let (r, rows) = audit_view(&gw, &analyst);
    assert!(!rows.is_empty(), "the analyst still sees the rows they may");
    for row in &rows {
        assert!(
            !field(row, "resource").contains(seed),
            "compartmented object leaked: {}",
            row.encode()
        );
    }
    assert_eq!(header(&r, "X-Audit-Filtered"), Some("true"));
}

/// A role that may Read anything but holds no ReadAudit rule is refused the ledger
/// view, and the refusal is itself a ledger row. [VLD-ANB-MIDGARD-0031] [AC-20.2]
#[test]
fn reading_the_audit_needs_the_read_audit_permission() {
    const NOW: u64 = 1_800_000_000;
    // The dev policy, less the operator's ReadAudit rules: the operator keeps Read on
    // `*`, the approver keeps ReadAudit and inspects the ledger.
    let rules: Vec<Rule> = dev_login_policy()
        .rules()
        .iter()
        .filter(|r| !(r.role.0 == "operator" && r.action.0 == ACTION_READ_AUDIT))
        .cloned()
        .collect();
    let mut rng = OsRng;
    let mut idp = Idp::generate("test-idp", &mut rng).expect("idp keygen");
    idp.store_mut().upsert("operator", demo_operator());
    idp.store_mut().upsert("approver", demo_approver());
    let issue = |idp: &Idp, who: &str| {
        let token = idp
            .issue_token(who, Purpose("operations".into()), NOW, 3_600)
            .expect("issue token");
        token.iter().fold(String::new(), |mut hex, b| {
            hex.push_str(&format!("{b:02x}"));
            hex
        })
    };
    let operator = issue(&idp, "operator");
    let approver = issue(&idp, "approver");
    let policy = PolicySet::new(rules).expect("valid policy");
    let governance = Governance::new(DefaultPdp::new(policy), &mut rng).expect("governance");
    let gw = SecuredGateway::with_enclosure(
        fips_provider(),
        idp.verifying_key().clone(),
        Enclosure::new(governance),
        vec![],
    );

    let r = call_at(&gw, "GET", "/api/v1/audit", "", "", Some(&operator), NOW);
    assert_eq!(r.status, 403, "{}", r.body);
    let r = call_at(&gw, "GET", "/api/v1/whoami", "", "", Some(&operator), NOW);
    assert_eq!(r.status, 200, "the operator still holds Read: {}", r.body);

    let r = call_at(
        &gw,
        "GET",
        "/api/v1/audit",
        "limit=500",
        "",
        Some(&approver),
        NOW,
    );
    assert_eq!(r.status, 200, "{}", r.body);
    let ledger = json(&r);
    assert!(
        items(&ledger)
            .iter()
            .any(|row| field(row, "actor") == "operator"
                && field(row, "action") == "DENY ReadAudit"
                && field(row, "resource") == "audit-ledger"),
        "no DENY ReadAudit row on audit-ledger: {}",
        ledger.encode()
    );
}

/// Login rows about other principals need ReadAudit on `audit-ledger:sessions`: the
/// approver sees only their own, the operator (who holds that grant in the dev policy)
/// sees both. [VLD-ANB-MIDGARD-0031] [AC-20.3]
#[test]
fn other_principals_login_rows_need_the_sessions_grant() {
    let (gw, operator) = session();
    let approver = approver(&gw);
    let session_actors = |token: &str| -> BTreeSet<String> {
        let (_, rows) = audit_view(&gw, token);
        rows.iter()
            .filter(|row| field(row, "resource") == "session")
            .map(|row| field(row, "actor").to_string())
            .collect()
    };

    let seen_by_approver = session_actors(&approver);
    assert_eq!(
        seen_by_approver,
        ["approver".to_string()].into_iter().collect(),
        "the approver should see only their own login rows"
    );
    let seen_by_operator = session_actors(&operator);
    assert_eq!(
        seen_by_operator,
        ["approver".to_string(), "operator".to_string()]
            .into_iter()
            .collect(),
        "the operator holds the sessions grant"
    );
}

/// The edge writes its PERMIT row before the enclosure checks the human gate, so a
/// refusal there is written too: a gated action without a token, and a requester
/// deciding their own approval, each leave a `REFUSED` row with its reason code and
/// target. [NFR-AI-1] [AC-M-4] [VLD-ANB-MIDGARD-0054] [AC-26.6]
#[test]
fn a_refused_human_gate_and_a_self_approval_are_in_the_ledger() {
    let (mut gw, token) = session();
    // One principal holding both roles, so the self-approval reaches the enclosure
    // rather than being denied by policy at the edge.
    gw.provision_login(
        "both",
        "both-password",
        iam::UserRecord {
            roles: [
                policy::Role("operator".into()),
                policy::Role("approver".into()),
            ]
            .into_iter()
            .collect(),
            ..http_gateway::demo_operator()
        },
    )
    .unwrap();

    let r = call(
        &gw,
        "POST",
        "/api/v1/actions",
        "",
        r#"{"actionType":"Ground aircraft","params":{"target":"aircraft/AC-2214"}}"#,
        Some(&token),
    );
    assert_eq!(r.status, 403, "{}", r.body);

    let both = login(&gw, "both", "both-password");
    let id = request_approval(&gw, &both, "Ground aircraft", "aircraft/AC-2214");
    let own = decide(&gw, &both, &id, "approve");
    assert_eq!(own.status, 403, "{}", own.body);

    let rows = audit(&gw, &token);
    let row = |actor: &str, action: &str| {
        items(&rows).iter().any(|r| {
            field(r, "actor") == actor
                && field(r, "action") == action
                && field(r, "resource") == "aircraft/AC-2214"
        })
    };
    assert!(
        row(DEMO_USERNAME, "REFUSED Apply (human-gate-missing)"),
        "{}",
        rows.encode()
    );
    assert!(
        row("both", "REFUSED Approve (self-approval)"),
        "{}",
        rows.encode()
    );
}

/// The row in `rows` with `action` on `resource`, if any.
fn audit_row<'a>(rows: &'a Json, action: &str, resource: &str) -> Option<&'a Json> {
    items(rows)
        .iter()
        .find(|r| field(r, "action") == action && field(r, "resource") == resource)
}

/// Who asked, who approved and which action ran on what are all in the signed ledger,
/// bound by the approval id: the request, the decision and the redemption each leave
/// an `APPROVAL` row on `approval:<id>`, and the fabric's own row for the action names
/// the approval and its approver. [NFR-AI-1] [AC-M-4] [VLD-ANB-MIDGARD-0038] [AC-21.1]
#[test]
fn the_ledger_binds_request_decision_and_redemption_to_the_approval() {
    let (gw, token) = session();
    let id = approved(&gw, &token, "Ground aircraft", "aircraft/AC-2214");
    assert_eq!(id, "APR-1");
    let r = act(&gw, &token, "Ground aircraft", "aircraft/AC-2214", &id);
    assert_eq!(r.status, 200, "{}", r.body);

    let rows = audit(&gw, &token);
    let lifecycle = [
        ("APPROVAL requested", DEMO_USERNAME, None),
        (
            "APPROVAL approved",
            DEMO_APPROVER_USERNAME,
            Some(DEMO_APPROVER_USERNAME),
        ),
        (
            "APPROVAL redeemed",
            DEMO_USERNAME,
            Some(DEMO_APPROVER_USERNAME),
        ),
    ];
    for (action, actor, approver) in lifecycle {
        let row = audit_row(&rows, action, "approval:APR-1")
            .unwrap_or_else(|| panic!("no {action} row: {}", rows.encode()));
        assert_eq!(field(row, "actor"), actor, "{}", row.encode());
        assert_eq!(field(row, "approvalId"), "APR-1", "{}", row.encode());
        assert_eq!(field(row, "target"), "aircraft/AC-2214", "{}", row.encode());
        assert_eq!(field(row, "reason"), "ground_aircraft", "{}", row.encode());
        assert_eq!(row.str_field("approver"), approver, "{}", row.encode());
    }

    let fabric_row = audit_row(&rows, "PERMIT groundAircraft", "ground_aircraft")
        .unwrap_or_else(|| panic!("no fabric action row: {}", rows.encode()));
    assert_eq!(field(fabric_row, "actor"), DEMO_USERNAME);
    assert_eq!(field(fabric_row, "approvalId"), "APR-1");
    assert_eq!(field(fabric_row, "approver"), DEMO_APPROVER_USERNAME);
    assert_eq!(field(fabric_row, "target"), "aircraft/AC-2214");
    assert_eq!(field(fabric_row, "outcome"), "permit");
}

/// A flood of approval requests cannot push a live gate out: each requester holds at
/// most 16 open requests, and once the store is full a request is refused with 429 and
/// a Retry-After rather than evicting a pending or approved entry, both of which still
/// work afterwards. [NFR-AI-1] [VLD-ANB-MIDGARD-0039] [AC-21.2]
#[test]
fn pending_and_approved_gates_are_not_evicted_by_a_flood_of_requests() {
    let (mut gw, token) = session();
    let pending = request_approval(&gw, &token, "Ground aircraft", "aircraft/AC-2214");
    let live = approved(&gw, &token, "Schedule maintenance", "aircraft/AC-2214");

    let flooders: Vec<String> = (0..16).map(|n| format!("flood-{n}")).collect();
    for name in &flooders {
        gw.provision_login(name, "flood-password", http_gateway::demo_operator())
            .unwrap();
    }
    let body = r#"{"actionType":"Ground aircraft","target":"aircraft/AC-2214"}"#;
    let mut accepted_by = Vec::new();
    let mut queue_full = None;
    for name in &flooders {
        let flood = login(&gw, name, "flood-password");
        let mut accepted = 0;
        let refusal = loop {
            let r = call(&gw, "POST", "/api/v1/approvals", "", body, Some(&flood));
            if r.status != 200 {
                break r;
            }
            accepted += 1;
            assert!(accepted <= 16, "more than 16 open requests for {name}");
        };
        assert_eq!(refusal.status, 429, "{}", refusal.body);
        assert_eq!(refusal.retry_after, Some(60), "{}", refusal.body);
        accepted_by.push(accepted);
        if refusal.body.contains("queue") {
            queue_full = Some(refusal);
            break;
        }
        assert!(
            refusal.body.contains("too many open approval requests"),
            "{}",
            refusal.body
        );
        assert_eq!(accepted, 16, "the per-requester cap is 16");
    }
    let queue_full = queue_full.expect("the store filled and refused");
    assert!(
        queue_full.body.contains("approval queue is full"),
        "{}",
        queue_full.body
    );
    // 256 in all: the operator's two and the flood's 254.
    assert_eq!(accepted_by.iter().sum::<usize>(), 254, "{accepted_by:?}");

    let listed = json(&call(&gw, "GET", "/api/v1/approvals", "", "", Some(&token)));
    let still_pending = items(&listed)
        .iter()
        .find(|a| field(a, "id") == pending)
        .expect("the pending approval is still listed");
    assert_eq!(field(still_pending, "status"), "pending");
    let r = act(
        &gw,
        &token,
        "Schedule maintenance",
        "aircraft/AC-2214",
        &live,
    );
    assert_eq!(r.status, 200, "the approved gate still redeems: {}", r.body);
}

/// A pending approval nobody decides expires after 24 hours: the next request prunes
/// it, it is no longer listed, and the ledger says it expired. [NFR-AI-1]
/// [VLD-ANB-MIDGARD-0039] [AC-21.3]
#[test]
fn a_pending_approval_expires_and_is_audited() {
    let (gw, token) = session();
    let first = request_approval(&gw, &token, "Ground aircraft", "aircraft/AC-2214");

    // A day and a second later, with a fresh session.
    let later = now() + 86_401;
    let body = format!(r#"{{"username":"{DEMO_USERNAME}","password":"{DEMO_PASSWORD}"}}"#);
    let r = call_at(&gw, "POST", "/api/v1/login", "", &body, None, later);
    assert_eq!(r.status, 200, "{}", r.body);
    let token = field(&json(&r), "token").to_string();
    let body = r#"{"actionType":"Ground aircraft","target":"aircraft/AC-2261"}"#;
    let r = call_at(
        &gw,
        "POST",
        "/api/v1/approvals",
        "",
        body,
        Some(&token),
        later,
    );
    assert_eq!(r.status, 200, "{}", r.body);

    let listed = call_at(&gw, "GET", "/api/v1/approvals", "", "", Some(&token), later);
    assert!(
        !items(&json(&listed))
            .iter()
            .any(|a| field(a, "id") == first),
        "{}",
        listed.body
    );
    let rows = call_at(
        &gw,
        "GET",
        "/api/v1/audit",
        "limit=200",
        "",
        Some(&token),
        later,
    );
    let rows = json(&rows);
    let expired = audit_row(&rows, "APPROVAL expired", &format!("approval:{first}"))
        .unwrap_or_else(|| panic!("no expiry row: {}", rows.encode()));
    assert_eq!(field(expired, "actor"), DEMO_USERNAME);
    assert_eq!(field(expired, "target"), "aircraft/AC-2214");
    assert_eq!(field(expired, "approvalId"), first);
}
