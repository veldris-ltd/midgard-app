//! Searching the fabric: comparisons beyond equality, text matching, ordering and
//! paging on `/query`, a clear refusal for a search the schema cannot answer, object
//! types that list their properties, and a search-around that says when it was cut
//! short. Every count and page covers only what the caller may read. [DD-1]
//! [NFR-SEC-2]
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
    body: &str,
    token: Option<&str>,
) -> Response {
    gw.handle(
        &Request {
            method: method.into(),
            path: path.into(),
            query: String::new(),
            body: body.into(),
            authorization: token.map(|t| format!("Bearer {t}")),
            origin: None,
        },
        now(),
    )
}

fn session() -> (SecuredGateway, String) {
    let gw = SecuredGateway::dev();
    let body = format!(r#"{{"username":"{DEMO_USERNAME}","password":"{DEMO_PASSWORD}"}}"#);
    let r = call(&gw, "POST", "/api/v1/login", &body, None);
    assert_eq!(r.status, 200, "{}", r.body);
    let token = parse(&r.body)
        .unwrap()
        .str_field("token")
        .expect("token")
        .to_string();
    (gw, token)
}

fn search(gw: &SecuredGateway, token: &str, body: &str) -> Json {
    let r = call(gw, "POST", "/api/v1/query", body, Some(token));
    assert_eq!(r.status, 200, "{body}: {}", r.body);
    parse(&r.body).unwrap()
}

fn ids(j: &Json) -> Vec<String> {
    match j.get("results") {
        Some(Json::Arr(rows)) => rows
            .iter()
            .map(|r| r.str_field("id").expect("id").to_string())
            .collect(),
        _ => panic!("no results: {}", j.encode()),
    }
}

fn int(j: &Json, key: &str) -> i64 {
    j.get(key)
        .and_then(Json::as_i64)
        .unwrap_or_else(|| panic!("no {key}: {}", j.encode()))
}

#[test]
fn a_range_condition_orders_and_pages_with_a_stable_total() {
    let (gw, token) = session();
    let body = |offset: i64, limit: i64| {
        format!(
            r#"{{"objectType":"aircraft","where":[{{"property":"flight_hours","op":"gt","value":"5000"}}],"orderBy":"flight_hours","descending":true,"offset":{offset},"limit":{limit}}}"#
        )
    };
    let all = search(&gw, &token, &body(0, 50));
    assert_eq!(
        ids(&all),
        ["aircraft/AC-2261", "aircraft/AC-2214", "aircraft/AC-2198"],
        "over 5,000 hours, most first"
    );
    assert_eq!(int(&all, "total"), 3);

    let first = search(&gw, &token, &body(0, 2));
    let second = search(&gw, &token, &body(2, 2));
    assert_eq!(ids(&first), ["aircraft/AC-2261", "aircraft/AC-2214"]);
    assert_eq!(ids(&second), ["aircraft/AC-2198"]);
    assert_eq!((int(&first, "total"), int(&second, "total")), (3, 3));
    assert_eq!(int(&second, "offset"), 2);
}

#[test]
fn conditions_combine_and_exact_filters_still_work() {
    let (gw, token) = session();
    let both = search(
        &gw,
        &token,
        r#"{"objectType":"aircraft","filter":{"base":"FAC-NORTH"},"where":[{"property":"flight_hours","op":"lte","value":"7000"}],"limit":50}"#,
    );
    assert_eq!(ids(&both), ["aircraft/AC-2214"]);

    let not = search(
        &gw,
        &token,
        r#"{"objectType":"aircraft","where":[{"property":"status","op":"ne","value":"active"}],"limit":50}"#,
    );
    assert_eq!(ids(&not), ["aircraft/AC-2198", "aircraft/AC-2307"]);
}

#[test]
fn text_matching_ignores_case_on_one_property_or_any() {
    let (gw, token) = session();
    let on_model = search(
        &gw,
        &token,
        r#"{"objectType":"aircraft","where":[{"property":"model","op":"contains","value":"HEAVY"}],"limit":50}"#,
    );
    assert_eq!(ids(&on_model), ["aircraft/AC-2198", "aircraft/AC-2214"]);

    let anywhere = search(
        &gw,
        &token,
        r#"{"objectType":"aircraft","where":[{"property":"*","op":"contains","value":"surveil"}],"limit":50}"#,
    );
    assert_eq!(ids(&anywhere), ["aircraft/AC-2307"]);
}

#[test]
fn counts_and_pages_cover_only_what_the_caller_may_read() {
    let (gw, token) = session();
    let feeds = search(&gw, &token, r#"{"objectType":"sensor_feed","limit":50}"#);
    assert_eq!(int(&feeds, "total"), 1, "the SIG feed is not counted");
    assert_eq!(ids(&feeds), ["sensor_feed/SF-TRANSPONDER-4"]);
}

#[test]
fn a_search_the_schema_cannot_answer_is_refused_with_the_reason() {
    let (gw, token) = session();
    for (body, status, says) in [
        (
            r#"{"objectType":"aircraft","where":[{"property":"wingspan","op":"gt","value":"1"}]}"#,
            400,
            "no property wingspan",
        ),
        (
            r#"{"objectType":"aircraft","where":[{"property":"flight_hours","op":"gt","value":"lots"}]}"#,
            400,
            "not a valid int",
        ),
        (
            r#"{"objectType":"aircraft","where":[{"property":"flight_hours","op":"contains","value":"1"}]}"#,
            400,
            "contains works on text",
        ),
        (
            r#"{"objectType":"aircraft","where":[{"property":"model","op":"like","value":"x"}]}"#,
            400,
            "unknown operator",
        ),
        (
            r#"{"objectType":"aircraft","orderBy":"wingspan"}"#,
            400,
            "to order by",
        ),
        (r#"{"objectType":"submarine"}"#, 404, "unknown object type"),
    ] {
        let r = call(&gw, "POST", "/api/v1/query", body, Some(&token));
        assert_eq!(r.status, status, "{body}: {}", r.body);
        assert!(r.body.contains(says), "{body}: {}", r.body);
    }
}

#[test]
fn object_types_list_their_properties() {
    let (gw, token) = session();
    let r = call(&gw, "GET", "/api/v1/object-types", "", Some(&token));
    assert_eq!(r.status, 200, "{}", r.body);
    assert!(
        r.body.contains(r#"{"name":"flight_hours","type":"int"}"#),
        "{}",
        r.body
    );
}

#[test]
fn search_around_says_when_it_was_cut_short() {
    let (gw, token) = session();
    let walk = |body: &str| {
        let r = call(&gw, "POST", "/api/v1/search-around", body, Some(&token));
        assert_eq!(r.status, 200, "{body}: {}", r.body);
        parse(&r.body).unwrap()
    };
    let nodes = |j: &Json| match j.get("nodes") {
        Some(Json::Arr(n)) => n.len(),
        _ => panic!("no nodes"),
    };

    let small = walk(r#"{"seedId":"aircraft/*","hops":2,"limit":2}"#);
    assert!(matches!(small.get("truncated"), Some(Json::Bool(true))));
    assert_eq!(nodes(&small), 2);
    assert_eq!(int(&small, "nodeLimit"), 2);
    assert_eq!(int(&small, "seedObjects"), 4);

    let whole = walk(r#"{"seedId":"aircraft/*","hops":4,"limit":500}"#);
    assert!(matches!(whole.get("truncated"), Some(Json::Bool(false))));
    assert!(nodes(&whole) > 4);

    let default = walk(r#"{"seedId":"aircraft/AC-2214","hops":1}"#);
    assert_eq!(int(&default, "nodeLimit"), 60);
}
