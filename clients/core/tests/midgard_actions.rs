//! Midgard's actions in the sample mode, ported from the release's `MockGateway`: each
//! succeeds as the mock answers, refuses as it refuses (and where the port adds a
//! refusal for governance), and is audited in the mock's numbering. Run through a real
//! session, so the core's own rules (targets the persona may see, one decision per
//! action, device-signed audit rows) hold around them. [FR-C-4] [AC-C-13] [NFR-SEC-2]
//! [NFR-AI-1] [DD-1]
#![cfg(feature = "backend-fips")]

use std::sync::atomic::{AtomicU64, Ordering};

use attest::json::{parse, Json};
use midgard_client_core::consoles::common::AuditRecord;
use midgard_client_core::consoles::midgard::{
    MidgardApproval, MidgardObjectInstance, MidgardObjectType, MidgardPolicyRule,
    MidgardSearchGraph,
};
use midgard_client_core::demo::canon;
use midgard_client_core::device_audit::DeviceAuditKey;
use midgard_client_core::keystore::SoftwareKeystore;
use midgard_client_core::marking::Visible;
use midgard_client_core::sample_source::{SampleSource, SAMPLE_JSON};
use midgard_client_core::session::{ActionState, Session, SessionError};
use pqc_agility::backend::fips::provider;
use pqc_agility::OsRng;

type S = Session<SoftwareKeystore, SampleSource>;

fn open(source: SampleSource, persona: &str) -> S {
    let ks = SoftwareKeystore::generate(&provider(), "device-root", &mut OsRng).unwrap();
    let (device, _) = DeviceAuditKey::create(&ks, &mut OsRng).unwrap();
    Session::open(ks, device, source, persona, "dev-1").unwrap()
}

fn session(persona: &str) -> S {
    open(SampleSource::new().unwrap(), persona)
}

fn switch(s: S, persona: &str) -> S {
    s.switch_persona(persona).map_err(|(_, e)| e).unwrap()
}

fn shown<T>(rows: Vec<Visible<T>>) -> Vec<T> {
    rows.into_iter()
        .filter_map(|v| match v {
            Visible::Shown { item, .. } => Some(item),
            Visible::Withheld { .. } => None,
        })
        .collect()
}

/// Request, sync and return the action's state.
fn act(s: &mut S, verb: &str, target: &str, params: &[(&str, &str)]) -> ActionState {
    let params: Vec<(String, String)> = params
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    let id = s.request_action(verb, target, &params).unwrap();
    s.sync().unwrap();
    s.action_state(&id)
}

fn applied(state: ActionState) -> (String, Json) {
    match state {
        ActionState::Applied { audit, result } => (audit, parse(&result).unwrap()),
        other => panic!("not applied: {other:?}"),
    }
}

fn refused(state: ActionState) -> String {
    match state {
        ActionState::Refused { reason } => reason,
        other => panic!("not refused: {other:?}"),
    }
}

fn str_of<'a>(v: &'a Json, key: &str) -> &'a str {
    v.get(key).and_then(Json::as_str).unwrap_or_default()
}

fn latest_audit(s: &mut S) -> AuditRecord {
    shown(s.rows::<AuditRecord>("midgard.audit").unwrap()).remove(0)
}

#[test]
fn the_dual_control_object_action_runs_end_to_end_with_two_personas() {
    // j.benjamin requests "Ground aircraft" on one aircraft.
    let mut s = session("j.benjamin");
    s.rows::<MidgardObjectInstance>("midgard.objects").unwrap();
    let (audit, approval) = applied(act(
        &mut s,
        "midgard.requestApproval",
        "midgard.objects/aircraft/1000",
        &[("actionType", "Ground aircraft")],
    ));
    assert_eq!(str_of(&approval, "id"), "APR-1");
    assert_eq!(str_of(&approval, "status"), "pending");
    assert_eq!(str_of(&approval, "target"), "aircraft/1000");
    assert_eq!(str_of(&approval, "requester"), "j.benjamin");
    assert_eq!(approval.get("mine"), Some(&Json::Bool(true)));
    // Audited as the mock numbers and stamps its rows, in the core's wording, signed by
    // this device.
    assert_eq!(audit, "AUD-1043");
    let row = latest_audit(&mut s);
    assert_eq!((row.id.as_str(), row.ts.as_str()), ("AUD-1043", "14:32:10"));
    assert_eq!(row.action, "applied · midgard.requestApproval");
    assert_eq!(row.resource, "midgard.objects/aircraft/1000");
    assert_eq!(row.actor, "j.benjamin");
    assert_eq!(row.pq_signature, "device");

    // Without the gate the action is refused, and the refusal is audited too.
    let why = refused(act(
        &mut s,
        "midgard.applyAction",
        "midgard.objects/aircraft/1000",
        &[("actionType", "Ground aircraft")],
    ));
    assert_eq!(why, "Human gate required for action \"Ground aircraft\"");
    assert_eq!(latest_audit(&mut s).action, "refused · midgard.applyAction");

    // The requester cannot approve their own request.
    let pending = shown(s.rows::<MidgardApproval>("midgard.approvals").unwrap());
    assert_eq!(pending.len(), 1);
    let why = refused(act(
        &mut s,
        "midgard.decideApproval",
        "midgard.approvals/APR-1",
        &[("approve", "true")],
    ));
    assert_eq!(
        why,
        "separation-of-duties: requester cannot approve own request"
    );

    // A second person (k.osei) approves it.
    let mut s = switch(s, "k.osei");
    s.rows::<MidgardApproval>("midgard.approvals").unwrap();
    let (_, decided) = applied(act(
        &mut s,
        "midgard.decideApproval",
        "midgard.approvals/APR-1",
        &[("approve", "true")],
    ));
    assert_eq!(str_of(&decided, "status"), "approved");
    assert_eq!(str_of(&decided, "approver"), "k.osei");
    assert!(decided.get("expiresAt").is_some());
    // Deciding it again is a conflict, as the mock's 409 is.
    match act(
        &mut s,
        "midgard.decideApproval",
        "midgard.approvals/APR-1",
        &[("approve", "false")],
    ) {
        ActionState::Conflict { reason } => {
            assert_eq!(reason, "approval APR-1 is already approved")
        }
        other => panic!("not a conflict: {other:?}"),
    }

    // Back to the requester, who runs it with the approval as the gate token, once.
    let mut s = switch(s, "j.benjamin");
    s.rows::<MidgardObjectInstance>("midgard.objects").unwrap();
    let (_, done) = applied(act(
        &mut s,
        "midgard.applyAction",
        "midgard.objects/aircraft/1000",
        &[
            ("actionType", "Ground aircraft"),
            ("humanGateToken", "APR-1"),
        ],
    ));
    assert_eq!(canon::encode(&done), r#"{"applied":true}"#);
    assert_eq!(latest_audit(&mut s).action, "applied · midgard.applyAction");
    let used = shown(s.rows::<MidgardApproval>("midgard.approvals").unwrap());
    assert_eq!(used[0].status, "used");
    let why = refused(act(
        &mut s,
        "midgard.applyAction",
        "midgard.objects/aircraft/1000",
        &[
            ("actionType", "Ground aircraft"),
            ("humanGateToken", "APR-1"),
        ],
    ));
    assert_eq!(why, "Human gate token is not a current approval of \"Ground aircraft\" on aircraft/1000 for this user");
}

#[test]
fn an_action_needing_no_gate_applies_and_unknown_or_ungated_requests_are_refused() {
    let mut s = session("k.osei");
    s.rows::<MidgardObjectInstance>("midgard.objects").unwrap();
    let (audit, _) = applied(act(
        &mut s,
        "midgard.applyAction",
        "midgard.objects/sensor_feed/1001",
        &[("actionType", "Recalibrate")],
    ));
    assert_eq!(audit, "AUD-1043");
    let why = refused(act(
        &mut s,
        "midgard.requestApproval",
        "midgard.objects/sensor_feed/1001",
        &[("actionType", "Recalibrate")],
    ));
    assert_eq!(why, "action \"Recalibrate\" needs no approval");
    let why = refused(act(
        &mut s,
        "midgard.applyAction",
        "midgard.objects/sensor_feed/1001",
        &[("actionType", "Launch")],
    ));
    assert_eq!(why, "unknown action type: Launch");
}

static NOW: AtomicU64 = AtomicU64::new(1_000_000);

fn clock() -> u64 {
    NOW.load(Ordering::SeqCst)
}

#[test]
fn an_approval_expires_after_its_time_to_live() {
    let mut s = open(SampleSource::with_clock(clock).unwrap(), "j.benjamin");
    s.rows::<MidgardObjectInstance>("midgard.objects").unwrap();
    applied(act(
        &mut s,
        "midgard.requestApproval",
        "midgard.objects/crew_member/1002",
        &[("actionType", "Update clearance")],
    ));
    let mut s = switch(s, "k.osei");
    s.rows::<MidgardApproval>("midgard.approvals").unwrap();
    applied(act(
        &mut s,
        "midgard.decideApproval",
        "midgard.approvals/APR-1",
        &[("approve", "true")],
    ));
    let mut s = switch(s, "j.benjamin");
    s.rows::<MidgardObjectInstance>("midgard.objects").unwrap();
    NOW.fetch_add(900, Ordering::SeqCst);
    let why = refused(act(
        &mut s,
        "midgard.applyAction",
        "midgard.objects/crew_member/1002",
        &[
            ("actionType", "Update clearance"),
            ("humanGateToken", "APR-1"),
        ],
    ));
    assert!(
        why.starts_with("Human gate token is not a current approval"),
        "{why}"
    );
}

#[test]
fn objects_above_a_persona_cannot_be_acted_on_or_queried() {
    // k.rao (restricted): aircraft and sensor feeds are withheld, so the session
    // refuses before anything is queued, as for a row that does not exist.
    let mut s = session("k.rao");
    s.rows::<MidgardObjectInstance>("midgard.objects").unwrap();
    s.rows::<MidgardObjectType>("midgard.objectTypes").unwrap();
    for (verb, target) in [
        ("midgard.requestApproval", "midgard.objects/aircraft/1000"),
        ("midgard.query", "midgard.objectTypes/sensor_feed"),
    ] {
        assert!(
            matches!(
                s.request_action(verb, target, &[]),
                Err(SessionError::AboveClearance)
            ),
            "{verb}"
        );
    }
}

#[test]
fn a_query_returns_what_the_persona_may_read_meeting_its_conditions() {
    let mut s = session("j.benjamin");
    s.rows::<MidgardObjectType>("midgard.objectTypes").unwrap();
    let (audit, all) = applied(act(
        &mut s,
        "midgard.query",
        "midgard.objectTypes/aircraft",
        &[("maxResults", "50")],
    ));
    assert_eq!(audit, "AUD-1043");
    assert_eq!(all.get("total"), Some(&Json::Num("5".into())));
    let row = latest_audit(&mut s);
    assert_eq!(
        (row.action.as_str(), row.resource.as_str()),
        ("applied · midgard.query", "midgard.objectTypes/aircraft")
    );

    // Any text, ignoring case, newest id first; paged.
    let (_, standby) = applied(act(
        &mut s,
        "midgard.query",
        "midgard.objectTypes/aircraft",
        &[
            (
                "where",
                r#"[{"op":"contains","property":"*","value":"STANDBY"}]"#,
            ),
            ("descending", "true"),
            ("offset", "1"),
            ("maxResults", "1"),
        ],
    ));
    assert_eq!(standby.get("total"), Some(&Json::Num("3".into())));
    let ids: Vec<&str> = standby
        .get("results")
        .and_then(Json::as_array)
        .unwrap()
        .iter()
        .map(|r| str_of(r, "id"))
        .collect();
    assert_eq!(ids, ["aircraft/1002"]);

    // The fabric's refusals.
    let why = refused(act(
        &mut s,
        "midgard.query",
        "midgard.objectTypes/aircraft",
        &[("where", r#"[{"op":"like","property":"*","value":"x"}]"#)],
    ));
    assert_eq!(
        why,
        "unknown operator \"like\": use eq, ne, lt, lte, gt, gte or contains"
    );
    let why = refused(act(
        &mut s,
        "midgard.query",
        "midgard.objectTypes/aircraft",
        &[("where", r#"[{"op":"eq","property":"*","value":"x"}]"#)],
    ));
    assert_eq!(why, "\"*\" (any text property) works only with contains");
}

#[test]
fn a_search_around_is_the_mocks_graph_less_what_the_persona_may_not_read() {
    // Golden: the port's graph for aircraft/1000 at two hops is the mock's own answer,
    // recorded in the sample data (`midgard.searchAround`), but for its audit id.
    let mut s = session("k.osei");
    s.rows::<MidgardObjectType>("midgard.objectTypes").unwrap();
    let (_, graph) = applied(act(
        &mut s,
        "midgard.searchAround",
        "midgard.objectTypes/aircraft",
        &[("seedId", "1000"), ("hops", "2")],
    ));
    let body = parse(SAMPLE_JSON).unwrap();
    let recorded = body
        .get("collections")
        .and_then(|c| c.get("midgard.searchAround"))
        .and_then(|c| c.get("rows"))
        .and_then(Json::as_array)
        .unwrap()
        .iter()
        .find(|r| str_of(r, "id") == "aircraft/1000")
        .unwrap()
        .get("item")
        .unwrap();
    for key in [
        "seed",
        "nodes",
        "edges",
        "truncated",
        "nodeLimit",
        "seedObjects",
    ] {
        assert_eq!(graph.get(key), recorded.get(key), "{key}");
    }
    assert_eq!(graph.get("withheld"), Some(&Json::Num("0".into())));
    assert_eq!(
        latest_audit(&mut s).action,
        "applied · midgard.searchAround"
    );
    // Less the withheld count, it decodes as the release's own read model (with an
    // audit id, which the action's audit row gives).
    let Json::Obj(mut pairs) = graph.clone() else {
        panic!("not an object")
    };
    pairs.retain(|(k, _)| k != "withheld");
    pairs.push(("auditId".into(), Json::Str("AUD-1043".into())));
    let _: MidgardSearchGraph =
        midgard_client_core::consoles::decode::FromJson::from_json(&Json::Obj(pairs), "$").unwrap();

    // k.rao (restricted) from a crew member: the confidential sensor feed is withheld.
    let mut s = session("k.rao");
    s.rows::<MidgardObjectType>("midgard.objectTypes").unwrap();
    let (_, graph) = applied(act(
        &mut s,
        "midgard.searchAround",
        "midgard.objectTypes/crew_member",
        &[("hops", "1")],
    ));
    assert_eq!(str_of(&graph, "seed"), "crew_member/*");
    assert_eq!(graph.get("withheld"), Some(&Json::Num("1".into())));
    let types: Vec<&str> = graph
        .get("nodes")
        .and_then(Json::as_array)
        .unwrap()
        .iter()
        .map(|n| str_of(n, "typeName"))
        .collect();
    assert_eq!(types, ["Crew Member", "Route", "Crew Member"]);
    assert_eq!(
        graph.get("edges").and_then(Json::as_array).unwrap().len(),
        2
    );
}

#[test]
fn a_policy_dry_run_is_deny_by_default_over_the_rules_the_persona_may_read() {
    let mut s = session("j.benjamin");
    s.rows::<MidgardPolicyRule>("midgard.policies").unwrap();
    let run = |s: &mut S, purpose: &str, marking: &str| {
        applied(act(
            s,
            "midgard.simulatePolicy",
            "midgard.policies/*",
            &[("purpose", purpose), ("marking", marking)],
        ))
    };
    let (audit, d) = run(&mut s, "operations", "confidential");
    assert_eq!(audit, "AUD-1043");
    assert_eq!(
        (str_of(&d, "decision"), str_of(&d, "matchedRule")),
        ("grant", "POL-CONF-OPS")
    );
    assert_eq!(str_of(&d, "reason"), "Granted by Confidential OPS-N read.");
    let (_, d) = run(&mut s, "export", "confidential");
    assert_eq!(
        (str_of(&d, "decision"), str_of(&d, "matchedRule")),
        ("deny", "POL-DENY-EXPORT")
    );
    assert_eq!(
        str_of(&d, "reason"),
        "Explicit deny by Deny egress of confidential."
    );
    let (_, d) = run(&mut s, "analysis", "restricted");
    assert_eq!(d.get("matchedRule"), Some(&Json::Null));
    assert_eq!(
        str_of(&d, "reason"),
        "Deny by default — no matching allow rule."
    );
    assert_eq!(latest_audit(&mut s).resource, "midgard.policies/*");

    // k.rao reads only the restricted rule, so a confidential question finds no allow.
    let mut s = session("k.rao");
    s.rows::<MidgardPolicyRule>("midgard.policies").unwrap();
    let (_, d) = run(&mut s, "operations", "confidential");
    assert_eq!(str_of(&d, "decision"), "deny");
    // A dry run names the rule set, not one rule.
    let why = refused(act(
        &mut s,
        "midgard.simulatePolicy",
        "midgard.policies/POL-RESTRICT",
        &[("purpose", "operations"), ("marking", "restricted")],
    ));
    assert_eq!(why, "a policy dry run is on midgard.policies/*");
}
