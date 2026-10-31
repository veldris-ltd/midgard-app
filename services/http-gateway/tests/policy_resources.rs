//! One resource-id scheme for policy rules on fabric data: `object-type:<type>` for a
//! type and `object:<type>/<key>` for one object, on every route that reads them. A
//! type-scoped deny holds on `/query`, on a search-around from that type and on the
//! objects a walk or an action would reach; an instance-scoped deny hides one object
//! everywhere; and the ledger names what was decided in the same terms.
//! [VLD-ANB-MIDGARD-0023] [AC-19.1] [AC-19.2] [AC-19.4] [NFR-SEC-2]

use governance::Governance;
use http_gateway::enclosure::Enclosure;
use http_gateway::http::{Request, Response};
use http_gateway::json::{parse, Json};
use http_gateway::{demo_operator, dev_login_policy, SecuredGateway};
use iam::Idp;
use policy::{
    Action, DefaultPdp, Effect, PolicySet, Purpose, ResourceId, Role, Rule, RuleId, Selector,
};
use pqc_agility::backend::fips::provider as fips_provider;
use pqc_agility::OsRng;

const NOW: u64 = 1_800_000_000;
const TTL: u64 = 3_600;
const PURPOSE: &str = "operations";

fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

/// The dev policy (the operator may Read anything, and act), plus a deny on Read of
/// each resource in `denied`.
fn policy(denied: &[&str]) -> PolicySet {
    let mut rules = dev_login_policy().rules().to_vec();
    rules.extend(denied.iter().enumerate().map(|(i, id)| Rule {
        id: RuleId(format!("deny-{i}")),
        role: Role("operator".into()),
        action: Action("Read".into()),
        purpose: None,
        selector: Selector::Resource(ResourceId((*id).into())),
        effect: Effect::Deny,
    }));
    PolicySet::new(rules).expect("valid policy")
}

/// A secured gateway over the seeded development data, enforcing `policy(denied)`,
/// and a token for the demo operator.
fn gateway(denied: &[&str]) -> (SecuredGateway, String) {
    let mut rng = OsRng;
    let mut idp = Idp::generate("test-idp", &mut rng).unwrap();
    idp.store_mut().upsert("operator", demo_operator());
    let token = idp
        .issue_token("operator", Purpose(PURPOSE.into()), NOW, TTL)
        .expect("issue token");
    let governance = Governance::new(DefaultPdp::new(policy(denied)), &mut rng).unwrap();
    let mut enclosure = Enclosure::new(governance);
    enclosure.seed_dev(PURPOSE, NOW).expect("dev data seeds");
    let gw = SecuredGateway::with_enclosure(
        fips_provider(),
        idp.verifying_key().clone(),
        enclosure,
        vec![],
    );
    (gw, hex(&token))
}

fn call(
    gw: &SecuredGateway,
    method: &str,
    path: &str,
    query: &str,
    body: &str,
    token: &str,
) -> Response {
    gw.handle(
        &Request {
            method: method.into(),
            path: path.into(),
            query: query.into(),
            body: body.into(),
            authorization: Some(format!("Bearer {token}")),
            origin: None,
        },
        NOW,
    )
}

fn query_aircraft(gw: &SecuredGateway, token: &str) -> Response {
    call(
        gw,
        "POST",
        "/api/v1/query",
        "",
        r#"{"objectType":"aircraft","limit":50}"#,
        token,
    )
}

fn search_around(gw: &SecuredGateway, token: &str, seed: &str) -> Response {
    let body = format!(r#"{{"seedId":"{seed}","hops":2}}"#);
    call(gw, "POST", "/api/v1/search-around", "", &body, token)
}

/// The `id` of every row under `key` (`results` or `nodes`).
fn ids(r: &Response, key: &str) -> Vec<String> {
    assert_eq!(r.status, 200, "{}", r.body);
    match parse(&r.body).unwrap().get(key) {
        Some(Json::Arr(rows)) => rows
            .iter()
            .map(|row| row.str_field("id").expect("id").to_string())
            .collect(),
        _ => panic!("no {key}: {}", r.body),
    }
}

#[test]
fn a_type_scoped_deny_holds_on_query_search_around_and_per_object_reads() {
    let (gw, token) = gateway(&["object-type:aircraft"]);

    assert_eq!(
        query_aircraft(&gw, &token).status,
        403,
        "the query is refused"
    );
    assert_eq!(
        search_around(&gw, &token, "aircraft/*").status,
        403,
        "a walk from every aircraft is refused"
    );

    let nodes = ids(&search_around(&gw, &token, "facility/FAC-NORTH"), "nodes");
    assert!(
        nodes.iter().any(|id| id == "facility/FAC-NORTH"),
        "the facility itself is served: {nodes:?}"
    );
    assert!(
        !nodes.iter().any(|id| id.starts_with("aircraft/")),
        "no aircraft is reached from the facility: {nodes:?}"
    );

    // Every dev action is human-gated; a token is supplied so the target is resolved,
    // and an aircraft the caller may not read is reported as unknown.
    let r = call(
        &gw,
        "POST",
        "/api/v1/actions",
        "",
        r#"{"actionType":"ground_aircraft","params":{"target":"aircraft/AC-2214"},"humanGateToken":"APR-1"}"#,
        &token,
    );
    assert_eq!(r.status, 404, "{}", r.body);
    assert!(r.body.contains("unknown target"), "{}", r.body);
}

#[test]
fn an_instance_scoped_deny_hides_one_object_everywhere() {
    let (gw, token) = gateway(&["object:aircraft/AC-2214"]);

    let r = query_aircraft(&gw, &token);
    let results = ids(&r, "results");
    assert!(
        !results.iter().any(|id| id == "aircraft/AC-2214"),
        "{results:?}"
    );
    assert!(
        results.iter().any(|id| id == "aircraft/AC-2261"),
        "{results:?}"
    );
    let total = parse(&r.body).unwrap().get("total").and_then(Json::as_i64);
    assert_eq!(
        total,
        Some(i64::try_from(results.len()).unwrap()),
        "total counts only visible rows"
    );

    let nodes = ids(&search_around(&gw, &token, "facility/FAC-NORTH"), "nodes");
    assert!(
        !nodes.iter().any(|id| id == "aircraft/AC-2214"),
        "{nodes:?}"
    );
    assert!(nodes.iter().any(|id| id == "aircraft/AC-2261"), "{nodes:?}");

    assert_eq!(
        search_around(&gw, &token, "aircraft/AC-2214").status,
        404,
        "the hidden object is an unknown seed"
    );
}

#[test]
fn the_audited_resource_ids_follow_the_scheme() {
    let (gw, token) = gateway(&[]);
    let before = gw.audit_len();
    assert_eq!(query_aircraft(&gw, &token).status, 200);
    assert_eq!(search_around(&gw, &token, "aircraft/*").status, 200);
    assert_eq!(search_around(&gw, &token, "aircraft/AC-2261").status, 200);
    let calls = gw.audit_len() - before;

    let r = call(&gw, "GET", "/api/v1/audit", "limit=500", "", &token);
    assert_eq!(r.status, 200, "{}", r.body);
    let rows = match parse(&r.body).unwrap() {
        Json::Arr(rows) => rows,
        other => panic!("not a list: {}", other.encode()),
    };
    // Newest first: the audit read itself, then the rows the three calls wrote.
    let resources: Vec<&str> = rows
        .iter()
        .skip(1)
        .take(calls)
        .map(|row| row.str_field("resource").expect("resource"))
        .collect();
    assert_eq!(calls, 6, "edge and enclosure each record the three calls");
    assert_eq!(
        resources,
        [
            "object:aircraft/AC-2261", // the single seed, in the enclosure
            "object-type:aircraft",    // its admission at the edge
            "object-type:aircraft",    // the wildcard walk, in the enclosure
            "object-type:aircraft",    // its admission at the edge
            "object-type:aircraft",    // the search, in the fabric
            "object-type:aircraft",    // its admission at the edge
        ]
    );
}
