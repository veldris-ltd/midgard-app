//! Bifrost's actions in the sample mode, ported from the release's `BifrostMock`: every
//! promotion step and rollback answers as the mock answered the golden calls
//! (`clients/demo/golden/<release>/bifrost.json`, recorded from the mock), with the mock's
//! audit numbering and stamps; the air-gap ring is refused in the mock's own words (fail
//! closed: nothing rolls); only the release-manager role promotes or rolls back; the
//! rings read back advanced; the audit rows are marked at the target's level, at least
//! restricted. Run through a real session, so the core's own rules hold around them.
//! [FR-C-4] [AC-C-13] [FR-B-2] [FR-B-5]
#![cfg(feature = "backend-fips")]

use attest::json::{parse, Json};
use midgard_client_core::consoles::bifrost::{BifrostRelease, BifrostRing, BifrostRollback};
use midgard_client_core::consoles::common::AuditRecord;
use midgard_client_core::demo::actions::bifrost::{bundle_required, role_required, ROLLBACK_HELD};
use midgard_client_core::demo::canon;
use midgard_client_core::device_audit::DeviceAuditKey;
use midgard_client_core::keystore::SoftwareKeystore;
use midgard_client_core::marking::Visible;
use midgard_client_core::sample_source::SampleSource;
use midgard_client_core::session::{ActionState, Session};
use pqc_agility::backend::fips::provider;
use pqc_agility::OsRng;

type S = Session<SoftwareKeystore, SampleSource>;

const GOLDEN: &str = include_str!("../../demo/golden/0.1.0-rc.3/bifrost.json");

fn session(persona: &str) -> S {
    let ks = SoftwareKeystore::generate(&provider(), "device-root", &mut OsRng).unwrap();
    let (device, _) = DeviceAuditKey::create(&ks, &mut OsRng).unwrap();
    let mut s = Session::open(ks, device, SampleSource::new().unwrap(), persona, "dev-1").unwrap();
    // An action is made on a row the session has read.
    s.rows::<BifrostRelease>("bifrost.releases").unwrap();
    s.rows::<BifrostRing>("bifrost.rings").unwrap();
    s.rows::<BifrostRollback>("bifrost.rollbacks").unwrap();
    s
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

fn act(s: &mut S, verb: &str, target: &str, params: &[(&str, &str)]) -> ActionState {
    let params: Vec<(String, String)> = params
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    let action = s.request_action(verb, target, &params).unwrap();
    s.sync().unwrap();
    s.action_state(&action)
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

/// `v` without `keys`.
fn without(v: &Json, keys: &[&str]) -> Json {
    let Json::Obj(pairs) = v else {
        panic!("not an object")
    };
    Json::Obj(
        pairs
            .iter()
            .filter(|(k, _)| !keys.contains(&k.as_str()))
            .cloned()
            .collect(),
    )
}

#[test]
fn promotions_and_rollbacks_answer_as_the_mock_did() {
    let golden = parse(GOLDEN).unwrap();
    assert_eq!(str_of(&golden, "format"), "midgard-golden");
    let audit = golden.get("audit").and_then(Json::as_array).unwrap();
    // The mock's record for an audit id: its wording and resource.
    let record = |id: &str| -> &Json {
        audit
            .iter()
            .find(|r| str_of(r, "id") == id)
            .unwrap_or_else(|| panic!("no golden record {id}"))
    };

    let mut s = session("a.holt");
    s.rows::<BifrostRing>("bifrost.rings").unwrap();
    s.rows::<BifrostRollback>("bifrost.rollbacks").unwrap();
    let promotions = golden.get("promote").and_then(Json::as_array).unwrap();
    assert_eq!(promotions.len(), 7);
    let mut refusals = 0;
    for p in promotions {
        let (release, ring) = (str_of(p, "release"), str_of(p, "ring"));
        let want = p.get("result").unwrap();
        let mock = record(str_of(want, "auditId"));
        let state = act(
            &mut s,
            "bifrost.promoteRelease",
            &format!("bifrost.releases/{release}"),
            &[("ring", ring)],
        );
        if str_of(want, "healthGate") == "fail" {
            // The courier-bundle gate: refused, in the mock's own record's words.
            let reason = refused(state);
            assert_eq!(reason, str_of(mock, "action"));
            assert_eq!(reason, bundle_required(release, ring));
            refusals += 1;
            continue;
        }
        let (audit_id, got) = applied(state);
        assert_eq!(audit_id, str_of(want, "auditId"), "{ring}");
        assert_eq!(
            canon::encode(&without(&got, &["step", "resource"])),
            canon::encode(&without(want, &["auditId"])),
            "{ring}"
        );
        assert_eq!(str_of(&got, "step"), str_of(mock, "action"));
        assert_eq!(str_of(&got, "resource"), str_of(mock, "resource"));
    }
    assert_eq!(refusals, 1);

    for r in golden.get("rollback").and_then(Json::as_array).unwrap() {
        let id = str_of(r, "id");
        let mock = record(str_of(r.get("result").unwrap(), "auditId"));
        let (audit_id, got) = applied(act(
            &mut s,
            "bifrost.rollback",
            &format!("bifrost.rollbacks/{id}"),
            &[],
        ));
        assert_eq!(audit_id, str_of(mock, "id"));
        assert_eq!(str_of(&got, "step"), str_of(mock, "action"));
        assert_eq!(str_of(&got, "plan"), str_of(mock, "resource"));
    }

    // The rings read back as the mock's did after the calls.
    let rings = shown(s.rows::<BifrostRing>("bifrost.rings").unwrap());
    let want_rings = golden.get("rings").and_then(Json::as_array).unwrap();
    assert_eq!(rings.len(), want_rings.len());
    for (got, want) in rings.iter().zip(want_rings) {
        assert_eq!(got.id, str_of(want, "id"));
        let done = match want.get("done") {
            Some(Json::Num(n)) => n.parse::<f64>().unwrap(),
            _ => panic!("no done"),
        };
        assert_eq!(got.done, done, "{}", got.id);
    }

    // A. Holt (internal) does not read the rows: they are marked restricted at least.
    let own = s.rows::<AuditRecord>("bifrost.audit").unwrap();
    assert_eq!(
        shown(own).len(),
        5,
        "only the seed (internal) is shown to a.holt"
    );

    // K. Osei (confidential) reads the ledger: the mock's numbering and stamps, newest
    // first, the persona the actor, the core's wording, the seed after.
    let mut s = switch(s, "k.osei");
    let rows = shown(s.rows::<AuditRecord>("bifrost.audit").unwrap());
    assert_eq!(rows.len(), audit.len() + 5);
    for (got, want) in rows.iter().zip(audit) {
        assert_eq!(got.id, str_of(want, "id"));
        assert_eq!(got.ts, str_of(want, "ts"));
        assert_eq!(got.actor, "a.holt");
    }
    assert_eq!(rows[0].action, "applied · bifrost.rollback");
    assert_eq!(rows[0].resource, "bifrost.rollbacks/RB-FORGE-42");
    assert_eq!(rows[3].action, "refused · bifrost.promoteRelease");
    assert_eq!(rows[audit.len()].id, "AUD-B-1042");
}

#[test]
fn only_a_release_manager_promotes_or_rolls_back() {
    for persona in ["k.rao", "j.benjamin", "k.osei"] {
        let mut s = session(persona);
        s.rows::<BifrostRing>("bifrost.rings").unwrap();
        let reason = refused(act(
            &mut s,
            "bifrost.promoteRelease",
            "bifrost.releases/REL-2050",
            &[("ring", "broad")],
        ));
        assert_eq!(reason, role_required("promote REL-2050"));
        assert_eq!(reason, "Role release-manager required to promote REL-2050");
        let reason = refused(act(
            &mut s,
            "bifrost.rollback",
            "bifrost.rollbacks/RB-REL-2050",
            &[],
        ));
        assert_eq!(
            reason,
            "Role release-manager required to roll back RB-REL-2050"
        );
        // Nothing rolled.
        let rings = shown(s.rows::<BifrostRing>("bifrost.rings").unwrap());
        assert_eq!(rings[2].done, 21.0, "{persona}");
    }
}

#[test]
fn unknown_rings_and_plans_are_refused_and_nothing_rolls() {
    let mut s = session("a.holt");
    s.rows::<BifrostRing>("bifrost.rings").unwrap();
    let reason = refused(act(
        &mut s,
        "bifrost.promoteRelease",
        "bifrost.releases/REL-2050",
        &[("ring", "orbit")],
    ));
    assert_eq!(reason, "no such ring: orbit");
    let reason = refused(act(
        &mut s,
        "bifrost.promoteRelease",
        "bifrost.releases/REL-2050",
        &[("ring", "air-gap")],
    ));
    assert_eq!(
        reason,
        "Promote · REL-2050 → ring air-gap (bundle required)"
    );
    let rings = shown(s.rows::<BifrostRing>("bifrost.rings").unwrap());
    assert_eq!(rings[3].done, 0.0);
    assert_eq!(rings[3].offline_bundle, Some(true));
    // A release or plan that is not there is no target at all.
    assert!(s
        .request_action(
            "bifrost.promoteRelease",
            "bifrost.releases/REL-9999",
            &[("ring".into(), "broad".into())]
        )
        .is_err());
    assert!(s
        .request_action("bifrost.rollback", "bifrost.rollbacks/RB-NONE", &[])
        .is_err());
    // The held gate's words are the web button's.
    assert_eq!(ROLLBACK_HELD, "Rollback held by gate");
}

#[test]
fn a_step_is_decided_once_and_a_re_push_does_not_roll_again() {
    let mut s = session("a.holt");
    s.rows::<BifrostRing>("bifrost.rings").unwrap();
    let action = s
        .request_action(
            "bifrost.promoteRelease",
            "bifrost.releases/REL-2050",
            &[("ring".into(), "broad".into())],
        )
        .unwrap();
    s.sync().unwrap();
    s.sync().unwrap();
    let (audit, result) = applied(s.action_state(&action));
    assert_eq!(audit, "AUD-B-1044");
    assert_eq!(result.get("progress"), Some(&Json::Num("60".into())));
    let rings = shown(s.rows::<BifrostRing>("bifrost.rings").unwrap());
    assert_eq!(rings[2].done, 29.0);
}
